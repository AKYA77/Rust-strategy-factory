'use strict';
const byId=id=>document.getElementById(id);
const el=(tag,text)=>{const n=document.createElement(tag);if(text!==undefined)n.textContent=String(text);return n;};
let csrf='',rows=[],exportText='',exportId='',currentEngine='';
const money=n=>{if(n===null||n===undefined)return '—';if(!Number.isSafeInteger(n))return 'Outside exact display range; export receipt';const value=BigInt(n),absolute=value<0n?-value:value;return '$'+(value<0n?'-':'')+(absolute/1000000n).toString()+'.'+(absolute%1000000n).toString().padStart(6,'0');};
const eventId=(n,exact)=>typeof exact==='string'&&/^(0|[1-9][0-9]{0,19})$/.test(exact)&&BigInt(exact)<=18446744073709551615n?exact:Number.isSafeInteger(n)?String(n):'Outside exact display range; export receipt';
const diagnosticJson=value=>JSON.stringify(value,(_key,value)=>typeof value==='number'&&Number.isInteger(value)&&!Number.isSafeInteger(value)?'See receipt export for exact integer':value,2);
function price(n,profile){const scale=profile?.price_scale;if(!Number.isSafeInteger(n)||!Number.isSafeInteger(scale)||scale<=0)return 'See exact receipt';const digits=Math.log10(scale);if(!Number.isInteger(digits))return n+'/'+scale;const value=BigInt(n),denom=BigInt(scale);return (value/denom).toString()+(digits?'.'+(value%denom).toString().padStart(digits,'0'):'');}
async function api(path,body){const response=await fetch(path,{method:body?'POST':'GET',headers:body?{'Content-Type':'application/json','X-CSRF-Token':csrf}:{},body:body?JSON.stringify(body):undefined});if(response.status===401&&path!=='/api/login'){location.assign('/');throw new Error('Sign in required');}if(!response.ok){let data;try{data=await response.json();}catch{data={error:'Request failed'};}throw new Error(data.error||'Request failed');}return response;}
function failure(error){byId('error').textContent=error.message;}
const form=byId('login');
if(form){form.addEventListener('submit',async e=>{e.preventDefault();byId('error').textContent='';try{await api('/api/login',{password:byId('password').value});byId('password').value='';location.assign('/');}catch(error){failure(error);}});}
else{
const symbols=['EURUSD','GBPUSD','USDJPY','USDCHF','USDCAD','AUDUSD','NZDUSD','GBPUSD'];
for(const symbol of symbols){const option=el('option',symbol);option.value=symbol;byId('instrument').append(option);const board=el('article');board.className='card';board.append(el('h3',symbol),el('p','No independently qualified strategies.'));byId('boards').append(board);}
let historyBefore=null,historyStack=[],historyNext=null,historyRequest=0;
function historyControls(){byId('history-previous').disabled=historyStack.length===0;byId('history-next').disabled=historyNext===null;}
async function loadHistory(){
 const ticket=++historyRequest,instrument=byId('instrument').value;
 const query=new URLSearchParams({limit:'25'});if(historyBefore!==null)query.set('before',historyBefore);if(instrument)query.set('instrument',instrument);
 historyNext=null;byId('history-previous').disabled=true;byId('history-next').disabled=true;byId('history-status').textContent='Loading experiments…';
 try{const page=await api('/api/experiment-history?'+query).then(r=>r.json());if(ticket!==historyRequest)return;rows=page.rows;historyNext=page.next_cursor;byId('history-status').textContent='Page '+(historyStack.length+1)+' · '+rows.length+' experiments';byId('history-status').dataset.instrument=instrument;renderResults();renderDatasets();}
 catch(error){if(ticket===historyRequest){byId('history-status').textContent='Unable to load experiments.';failure(error);}}
 finally{if(ticket===historyRequest)historyControls();}
}
function newestHistory(){historyBefore=null;historyStack=[];return loadHistory();}
byId('instrument').addEventListener('change',newestHistory);
byId('history-latest').addEventListener('click',newestHistory);
byId('history-next').addEventListener('click',()=>{if(historyNext===null)return;historyStack.push(historyBefore);historyBefore=historyNext;loadHistory();});
byId('history-previous').addEventListener('click',()=>{if(!historyStack.length)return;historyBefore=historyStack.pop();loadHistory();});

byId('logout').addEventListener('click',()=>api('/api/logout',{}).then(()=>location.assign('/')).catch(failure));
byId('export').addEventListener('click',()=>{const link=el('a');const url=URL.createObjectURL(new Blob([exportText],{type:'application/json'}));link.href=url;link.download=exportId+'.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});
function renderResults(){const table=byId('results');table.replaceChildren();for(const row of rows){const tr=el('tr');const button=el('button',row.id.slice(0,12));button.addEventListener('click',()=>detail(row).catch(failure));const id=el('td');id.append(button);if(row.key.protocol_hash==='synthetic-corpus-training-v1'){const recovery=el('button','Recovery');recovery.addEventListener('click',()=>loadCorpus(row.id));id.append(recovery);}tr.append(id,...[row.key.instrument,row.strategy?.timeframe||'—',row.state+(row.key.engine_version!==currentEngine?' · legacy engine':''),row.closed_trades??'—',money(row.ending_micro_usd)].map(v=>el('td',v)));table.append(tr);}}
let corpusSelected='',corpusRequest=0,corpusRaw='';
function corpusDisable(){for(const action of ['pause','resume','stop'])byId('corpus-'+action).disabled=true;}
async function loadCorpus(id){
 const request=++corpusRequest;corpusSelected=id;corpusRaw='';corpusDisable();byId('corpus-export').disabled=true;byId('corpus-recovery').hidden=false;byId('corpus-label').textContent=id;byId('corpus-status').textContent='Verifying recovery checkpoint…';byId('corpus-json').textContent='';
 try{const raw=await api('/api/corpus/'+id+'/status').then(r=>r.text());if(request!==corpusRequest)return;const data=JSON.parse(raw);corpusRaw=raw;byId('corpus-json').textContent=diagnosticJson(data);byId('corpus-status').textContent=data.state+' · '+data.control+' · checkpoint '+(data.checkpoint_cursor??'none')+' · attempt '+data.attempt;byId('corpus-export').disabled=false;
 const active=['queued','running'].includes(data.state)&&data.control!=='stopped';byId('corpus-pause').disabled=!active||data.control!=='running';byId('corpus-resume').disabled=!active||data.control!=='paused';byId('corpus-stop').disabled=!active;
 }catch(error){if(request===corpusRequest){byId('corpus-status').textContent='Verified recovery status unavailable.';failure(error);}}
}
for(const [action,state]of [['pause','paused'],['resume','running'],['stop','stopped']])byId('corpus-'+action).addEventListener('click',async()=>{
 const id=corpusSelected;corpusDisable();try{await api('/api/corpus/'+id+'/control',{state});if(id===corpusSelected)await loadCorpus(id);}catch(error){if(id===corpusSelected){await loadCorpus(id);failure(error);}}
});
byId('corpus-refresh').addEventListener('click',()=>{if(corpusSelected)loadCorpus(corpusSelected);});
byId('corpus-export').addEventListener('click',()=>{if(!corpusRaw)return;const link=el('a'),url=URL.createObjectURL(new Blob([corpusRaw],{type:'application/json'}));link.href=url;link.download='corpus-recovery-'+corpusSelected+'.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});

let trainingSelection='',trainingDiagnosis='',trainingText='',trainingId='';
async function trainingList(kind,parent,host,before=null,append=false,owner=exportId,selection=null){
 const query=new URLSearchParams({parent,limit:'10'});if(before!==null)query.set('before',before);
 const page=await api('/api/training-'+kind+'?'+query).then(r=>r.json());
 if(owner!==exportId||(selection!==null&&selection!==trainingDiagnosis))return;
 if(!append)host.replaceChildren();
 if(!page.rows.length&&!append)host.append(el('p',kind==='diagnoses'?'No stored training diagnoses for this experiment.':'No stored comparable reviews for this diagnosis.'));
 for(const row of page.rows){const button=el('button',(kind==='diagnoses'?'Diagnosis ':'Review ')+row.id.slice(0,12)+' · '+row.children+' children · '+row.status);button.addEventListener('click',()=>showTraining(kind,row.id).catch(failure));host.append(button);}
 if(page.next_cursor!==null){const more=el('button','More '+kind);more.addEventListener('click',()=>{more.remove();trainingList(kind,parent,host,page.next_cursor,true,owner,selection).catch(failure);});host.append(more);}
}
async function showTraining(kind,id){
 const owner=exportId;trainingSelection=id;if(kind==='diagnoses'){trainingDiagnosis=id;byId('training-reviews').replaceChildren();}
 const text=await api('/api/training-'+kind+'/'+id).then(r=>r.text());
 if(owner!==exportId||trainingSelection!==id)return;
 trainingText=text;trainingId=id;byId('training-evidence').hidden=false;byId('training-json').textContent=diagnosticJson(JSON.parse(text));
 if(kind==='diagnoses')await trainingList('reviews',id,byId('training-reviews'),null,false,owner,id);
}
byId('training-export').addEventListener('click',()=>{const link=el('a'),url=URL.createObjectURL(new Blob([trainingText],{type:'application/json'}));link.href=url;link.download='training-'+trainingId+'.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});
let detailRequest=0,tradeRequest=0,tradeAfter=null,tradeNext=null,tradeStack=[],tradeProfile=null,tradeExpectedHash='',tradeNoReplay='';
async function loadTrades(){
 const request=++tradeRequest,owner=exportId,expected=tradeExpectedHash,profile=tradeProfile;tradeNext=null;byId('trade-previous').disabled=true;byId('trade-next').disabled=true;const query=new URLSearchParams({limit:'100'});if(tradeAfter!==null)query.set('after',tradeAfter);
 try{const page=await api('/api/experiments/'+owner+'/trades?'+query).then(r=>r.json());if(request!==tradeRequest||owner!==exportId||expected!==tradeExpectedHash)return;if(page.result_hash!==expected)throw new Error('Receipt changed; select the experiment again.');tradeNext=page.next_cursor;const body=byId('trades');body.replaceChildren();
 for(const row of page.rows){const t=row.trade,tr=el('tr');tr.append(...[eventId(t.entry_event,row.event_ids.entry),eventId(t.exit_event,row.event_ids.exit),t.side,price(t.entry,profile),price(t.exit,profile),money(t.net_micro_usd),t.reason].map(v=>el('td',v)));body.append(tr);}
 byId('trade-count').textContent=!page.replay_present?tradeNoReplay:page.rows.length?'Showing '+page.first_ordinal+'–'+page.last_ordinal+' of '+page.total_trades+' resolved trades. Full receipt includes pending orders, rejections and provenance.':'No resolved trades in this receipt.';
 }catch(error){if(request===tradeRequest&&owner===exportId){byId('trades').replaceChildren();byId('trade-count').textContent='Unable to verify trade page.';failure(error);}}
 finally{if(request===tradeRequest&&owner===exportId){byId('trade-previous').disabled=!tradeStack.length;byId('trade-next').disabled=tradeNext===null;}}
}
byId('trade-next').addEventListener('click',()=>{if(tradeNext!==null){tradeStack.push(tradeAfter);tradeAfter=tradeNext;loadTrades();}});
byId('trade-previous').addEventListener('click',()=>{if(tradeStack.length){tradeAfter=tradeStack.pop();loadTrades();}});
async function detail(row){const request=++detailRequest;tradeRequest++;byId('detail').hidden=true;byId('export').disabled=true;const response=await api('/api/experiments/'+row.id);const raw=await response.text();if(request!==detailRequest)return;exportText=raw;exportId=row.id;trainingSelection='';trainingDiagnosis='';trainingText='';trainingId='';byId('training-evidence').hidden=true;byId('training-diagnoses').replaceChildren();byId('training-reviews').replaceChildren();const receipt=JSON.parse(exportText),data=receipt.result;byId('detail-label').textContent=row.key.instrument+' · '+(data.data_manifest?.class||'Data classification unavailable')+' · '+row.id+(row.key.engine_version!==currentEngine?' · Legacy engine: fresh verification required':'');byId('strategy').textContent=diagnosticJson({strategy:data.strategy,instrument_profile:data.instrument_profile,cost_profile:data.cost_profile,numeric_training_checks:receipt.numeric_training_assessment||receipt.assessment_unavailable_reason,screen:data.screen,conditional_barrier_audit:data.screen_audit,candidate_m1_shadow_audit:data.candidate_screen_audit,indexed_training_path:data.indexed_policy?{policy:data.indexed_policy,execution:data.indexed_replay}:undefined,market_entry_proof:data.market_stop_proof,proof_screening:{rejected:data.proof_rejection,audited:data.proof_audited,policy:data.proof_policy},profile:row.key.profile_hash,dataset:row.key.dataset_hash,engine:row.key.engine_version});const trades=data.tick_replay?.trades||[];tradeAfter=null;tradeNext=null;tradeStack=[];tradeProfile=data.instrument_profile;tradeExpectedHash=receipt.result_hash;tradeNoReplay=data.proof_rejection===true&&data.tick_replay==null?'No trades simulated: fixed market entry is mathematically infeasible under this specification. The receipt includes its verified proof.':'No tick replay was recorded. Full receipt retains the result evidence.';chart(trades);await loadTrades();if(request!==detailRequest)return;byId('export').disabled=false;byId('detail').hidden=false;byId('detail').scrollIntoView({behavior:'smooth'});await trainingList('diagnoses',row.id,byId('training-diagnoses'));}
function chart(trades){
 const host=byId('equity-chart');host.replaceChildren();if(!trades.length){host.append(el('p','No resolved equity series.'));return;}
 let value=20000000,min=value,max=value,low=null,high=null;const points=[[0,value]],bucket=Math.max(1,Math.ceil(trades.length/500));const add=pair=>{if(points.at(-1)[0]!==pair[0])points.push(pair);};
 for(let i=0;i<trades.length;i++){
  const net=trades[i].net_micro_usd;if(!Number.isSafeInteger(net)||!Number.isSafeInteger(value+net)){host.append(el('p','Equity exceeds exact chart range; export the integer receipt.'));return;}value+=net;min=Math.min(min,value);max=Math.max(max,value);const point=[i+1,value];if(low===null||value<low[1])low=point;if(high===null||value>high[1])high=point;
  if((i+1)%bucket===0||i===trades.length-1){for(const p of [low,high].sort((a,b)=>a[0]-b[0]))add(p);low=null;high=null;}
 }
 add([trades.length,value]);const range=Math.max(1,max-min);if(!Number.isSafeInteger(range)){host.append(el('p','Equity exceeds exact chart range; export the integer receipt.'));return;}
 const svg=document.createElementNS('http://www.w3.org/2000/svg','svg');svg.setAttribute('viewBox','0 0 1000 180');svg.setAttribute('role','img');svg.setAttribute('aria-label','Realized hypothetical equity from persisted trades');const line=document.createElementNS(svg.namespaceURI,'polyline');line.setAttribute('points',points.map(([i,v])=>(10+i/trades.length*980)+','+(165-(v-min)/range*150)).join(' '));svg.append(line);host.append(el('h3','Realized hypothetical equity'),svg,el('p','Starting '+money(20000000)+' · Ending '+money(value)),el('p','Chronological bucket extrema · '+points.length+' chart points from '+trades.length+' resolved trades. Full series remains in the exact receipt export.'));
}
let catalogAfter=null,catalogNext=null,catalogStack=[],catalogRequest=0,catalogDetailRequest=0,catalogSelected='',catalogRaw='';
function catalogControls(){byId('catalog-next').disabled=catalogNext===null;byId('catalog-previous').disabled=!catalogStack.length;}
async function loadCatalog(){
 const request=++catalogRequest,after=catalogAfter;
 try{
  const query=new URLSearchParams({limit:'25'});if(after!==null)query.set('after',after);
  const page=await api('/api/catalog?'+query).then(r=>r.json());if(request!==catalogRequest)return;
  const list=byId('catalog-entries');list.replaceChildren();catalogNext=page.next_cursor??null;catalogControls();
  const status=byId('catalog-status');status.dataset.after=after??'';
  status.textContent=page.configured?'Showing '+page.entries.length+' catalog observations in identity order. '+page.limitations:'No dataset catalog configured. Experiment-page provenance is separate.';
  for(const entry of page.entries){const f=entry.receipt.facts,card=el('article');card.className='card';card.append(el('h3',f.profile.symbol+' · '+f.dataset_identity.slice(0,12)),el('p','Declared class: '+f.declared_class),el('p','Observed events: '+f.events),el('p','First: '+new Date(f.first_ms).toISOString()+' · sequence '+eventId(null,f.first_sequence)),el('p','Latest: '+new Date(f.last_ms).toISOString()+' · sequence '+eventId(null,f.last_sequence)),el('p','Declared source: '+f.source));const button=el('button','Inspect catalog '+f.dataset_identity.slice(0,12));button.addEventListener('click',()=>catalogDetail(f.dataset_identity).catch(failure));card.append(button);list.append(card);}
 }catch(error){if(request!==catalogRequest)return;catalogNext=null;catalogControls();byId('catalog-entries').replaceChildren();byId('catalog-status').textContent='Catalog unavailable; observations could not be verified.';catalogSelected='';catalogRaw='';byId('catalog-detail').hidden=true;byId('data-review').hidden=true;clearLiveReview();failure(error);}
}
async function catalogDetail(id){const request=++catalogDetailRequest;catalogSelected=id;catalogRaw='';byId('catalog-detail').hidden=true;byId('data-review').hidden=true;const raw=await api('/api/catalog/'+id).then(r=>r.text());if(catalogSelected!==id||request!==catalogDetailRequest)return;catalogRaw=raw;byId('catalog-detail').hidden=false;byId('catalog-detail-label').textContent=id+' · local observation; no independent approval';byId('catalog-json').textContent=diagnosticJson(JSON.parse(raw));byId('catalog-detail').scrollIntoView({behavior:'smooth'});await loadDataReview(id);}
byId('catalog-first').addEventListener('click',()=>{catalogAfter=null;catalogStack=[];loadCatalog();});
byId('catalog-next').addEventListener('click',()=>{if(catalogNext===null)return;catalogStack.push(catalogAfter);catalogAfter=catalogNext;loadCatalog();});
byId('catalog-previous').addEventListener('click',()=>{if(!catalogStack.length)return;catalogAfter=catalogStack.pop();loadCatalog();});
byId('catalog-export').addEventListener('click',()=>{if(!catalogRaw)return;const link=el('a'),url=URL.createObjectURL(new Blob([catalogRaw],{type:'application/json'}));link.href=url;link.download='catalog-'+catalogSelected+'.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});
function renderAuditHealth(health){const panel=byId('audit-health');panel.replaceChildren();for(const [field,label] of [['proof_blocked','Proof screening'],['indexed_blocked','Indexed replay']]){const value=health?.[field];panel.append(el('p',label+': '+(value===true?'BLOCKED — persisted audit failure requires investigation':value===false?'No persisted failure marker':'Status unavailable')));}panel.append(el('p','Registry-local failure monitoring. No marker does not establish audit coverage, correctness or qualification.'));}
function ledgerPager(kind,render){
 let before=null,next=null,stack=[],request=0;
 async function load(){const ticket=++request;next=null;byId(kind+'-previous').disabled=true;byId(kind+'-next').disabled=true;byId(kind+'-status').textContent='Loading '+kind+' history…';const query=new URLSearchParams({limit:'25'});if(before!==null)query.set('before',before);
  try{const data=await api('/api/'+kind+'-history?'+query).then(r=>r.json());if(ticket!==request)return;next=data.next_cursor;render(data.rows);byId(kind+'-status').textContent='Page '+(stack.length+1)+' · '+data.rows.length+' records';}
  catch(error){if(ticket===request){render([]);byId(kind+'-status').textContent='Unable to verify '+kind+' history.';failure(error);}}
  finally{if(ticket===request){byId(kind+'-previous').disabled=!stack.length;byId(kind+'-next').disabled=next===null;}}
 }
 byId(kind+'-latest').addEventListener('click',()=>{before=null;stack=[];load();});
 byId(kind+'-previous').addEventListener('click',()=>{if(stack.length){before=stack.pop();load();}});
 byId(kind+'-next').addEventListener('click',()=>{if(next!==null){stack.push(before);before=next;load();}});
 return {load};
}

let protocolRequest=0,protocolSelected='',protocolRaw='';
function resetFoldMoney(text){if(typeof text!=='string'||!/^(-?)(0|[1-9][0-9]{0,38})$/.test(text))return 'See exact report';const n=BigInt(text),v=n<0n?-n:n;return (n<0n?'-$':'$')+(v/1000000n).toString()+'.'+(v%1000000n).toString().padStart(6,'0');}
async function loadProtocol(id){const ticket=++protocolRequest;protocolSelected=id;protocolRaw='';byId('protocol-evidence-export').disabled=true;byId('protocol-evidence').hidden=false;byId('protocol-detail').hidden=false;byId('protocol-detail').textContent='Verifying frozen report…';byId('protocol-evidence-cards').replaceChildren();byId('protocol-blockers').replaceChildren();byId('protocol-evidence-status').textContent='Verifying…';try{const [raw,summary]=await Promise.all([api('/api/protocols/'+id).then(r=>r.text()),api('/api/protocols/'+id+'/summary').then(r=>r.json())]);if(ticket!==protocolRequest)return;protocolRaw=raw;byId('protocol-detail').textContent=raw;byId('protocol-evidence-status').textContent=summary.protocol_id+' · '+summary.status+' · '+summary.declared_data_class+' · declared trials '+summary.declared_trials+' · no independent qualification';byId('protocol-evidence-export').disabled=false;for(const text of summary.missing_requirements)byId('protocol-blockers').append(el('li',text));for(const c of summary.candidates){const card=el('article');card.className='card';card.append(el('h4',c.concept_id.slice(0,12)),el('p',c.closed+' OOS resolved · '+c.wins+' net wins · '+c.losses+' net losses · '+c.breakevens+' breakevens'),el('p','Reset-fold net '+resetFoldMoney(c.reset_fold_net_micro_usd)+' · unresolved '+c.unresolved_oos_positions+' · failed windows '+c.failed_windows));if(c.conditional_evidence){const d=c.conditional_evidence;card.append(el('p','Conditional final-horizon criterion: '+(d.fixed_horizon_gate_passed?'met':'held')+' · '+d.observations+' outcomes · family '+d.declared_trials),el('p','Log evidence '+d.log_final_evidence+' · threshold '+d.log_threshold));}else card.append(el('p','Conditional evidence policy was not frozen.'));if(c.evidence_gate){card.append(el('p',c.evidence_gate.state));const list=el('ul');for(const d of c.evidence_gate.decisions)list.append(el('li',(d.passed?'Met: ':'Held: ')+d.criterion+' · '+d.explanation));card.append(list);for(const blocker of c.evidence_gate.remaining_blockers)card.append(el('p',blocker));}else card.append(el('p','Combined evidence policy was not frozen.'));if(c.robustness)card.append(el('p','Frozen robustness: '+c.robustness.state+' · '+c.robustness.cases+' cases'));byId('protocol-evidence-cards').append(card);}}
catch(error){if(ticket===protocolRequest){protocolRaw='';byId('protocol-evidence-export').disabled=true;byId('protocol-detail').textContent='Verified frozen report unavailable.';byId('protocol-evidence-status').textContent='Verified evidence unavailable.';failure(error);}}}
function renderProtocols(records){byId('protocols').replaceChildren();if(!records.length)byId('protocols').append(el('p','No frozen protocols registered.'));for(const p of records){const button=el('button',p.id.slice(0,12)+(p.report_ready?' · inspect evidence':' · incomplete'));button.dataset.protocolId=p.id;button.disabled=!p.report_ready;button.addEventListener('click',()=>loadProtocol(p.id));byId('protocols').append(button);}}
byId('protocol-evidence-refresh').addEventListener('click',()=>{if(protocolSelected)loadProtocol(protocolSelected);});byId('protocol-evidence-export').addEventListener('click',()=>{if(!protocolRaw)return;const link=el('a'),url=URL.createObjectURL(new Blob([protocolRaw],{type:'application/json'}));link.href=url;link.download='protocol-'+protocolSelected+'.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});

function renderRuns(records){byId('runs').replaceChildren();if(!records.length)byId('runs').append(el('p','No registered research runs.'));for(const run of records){const card=el('article');card.className='card';card.append(el('h3',run.id.slice(0,12)),el('p',run.state+' · '+(run.active?'task lease active':'no active task lease')+' · cursor '+run.cursor),el('p','Base tick simulations '+run.executed+' · reused '+run.reused+' · rejected '+run.rejected+' · failed '+run.failed));if(run.improvements)card.append(el('p','Improvement jobs queued '+run.improvements.queued+' · completed '+run.improvements.completed+' · failed '+run.improvements.failed+' · child receipt links '+run.improvements.child_receipt_links));if(run.last_error)card.append(el('p',run.last_error));const controls=el('div');controls.className='controls';for(const [label,state]of [['Pause','paused'],['Resume','running'],['Stop','stopped']]){const button=el('button',label);button.addEventListener('click',()=>api('/api/runs/'+run.id+'/control',{state}).then(refresh).catch(failure));controls.append(button);}card.append(controls);byId('runs').append(card);}}
const protocolPager=ledgerPager("protocol",renderProtocols),runPager=ledgerPager("run",renderRuns);
let retryRaw='',retryRequest=0;
let generatedSearchRequest=0,generatedDetailRequest=0,generatedSelected='',generatedRaw='';
async function loadGeneratedSearches(){
 const request=++generatedSearchRequest;try{const rows=await api('/api/corpus-searches').then(r=>r.json());if(request!==generatedSearchRequest)return;const host=byId('generated-search-cards');host.replaceChildren();byId('generated-search-status').textContent=rows.length+' registered generated searches. Slot counts are not new tick simulations.';
 for(const search of rows){const card=el('article');card.className='card';card.dataset.searchId=search.search_id;card.append(el('h3',search.search_id.slice(0,12)),el('p',search.state+' · generated slots '+search.generated_slots+' of '+search.maximum_candidates),el('p',search.lease_active?'Coordinator lease active':'No active coordinator lease'),el('p','State epoch '+(search.control?.epoch??'unavailable')+' · pause origin '+(search.control?.pause_origin??'none')),el('p',search.control?.history_complete?'Transition detail complete since control tracking began':'Transition detail omitted: '+(search.control?.omitted_transitions??'unavailable')),el('p','Completed slots '+(search.slot_states.completed??0)+' · pending '+((search.slot_states.queued??0)+(search.slot_states.pending??0))+' · errors '+(search.slot_states.error??0)),el('p','Recent error kinds: '+([...new Set((search.recent_slots??[]).filter(j=>j.state==='error').map(j=>j.error_kind??'legacy_unknown'))].join(', ')||'none')+' · no dashboard/background worker launched'));const controls=el('div');controls.className='controls';
 for(const [label,state]of [['Pause generated search','paused'],['Resume generated search','running'],['Stop generated search permanently','stopped']]){const button=el('button',label);button.disabled=['stopped','exhausted'].includes(search.state)||(state==='paused'&&search.state!=='running')||(state==='running'&&search.state!=='paused');button.addEventListener('click',async()=>{button.disabled=true;try{await api('/api/corpus-searches/'+search.search_id+'/control',{state});await loadGeneratedSearches();if(generatedSelected===search.search_id)await loadGeneratedSearchDetail(search.search_id);}catch(error){await loadGeneratedSearches();failure(error);}});controls.append(button);}
 const requeue=el('button','Requeue eligible error slots');requeue.disabled=search.state!=='paused'||search.lease_active||BigInt(search.retry_eligible_slots??'0')===0n||BigInt(search.retry_capacity??'0')===0n;requeue.addEventListener('click',async()=>{requeue.disabled=true;try{await api('/api/corpus-searches/'+search.search_id+'/requeue',{maximum_slots:32});await loadGeneratedSearches();if(generatedSelected===search.search_id)await loadGeneratedSearchDetail(search.search_id);}catch(error){await loadGeneratedSearches();failure(error);}});controls.append(requeue);card.append(el('p','Requeue records '+(search.requeue_records??'0')+' · original errors remain in history.'));
 const policy=search.retry_policy;const backoff=el('input'),slots=el('input');backoff.type=slots.type='number';backoff.min=slots.min='1';backoff.max='86400000';slots.max='32';backoff.value=policy?.backoff_ms??'1000';slots.value=policy?.maximum_slots??'1';backoff.disabled=slots.disabled=!!policy;const backoffLabel=el('label','Retry backoff (ms)'),slotsLabel=el('label','Maximum retry slots');backoffLabel.append(backoff);slotsLabel.append(slots);card.append(backoffLabel,slotsLabel,el('p',policy?'Frozen retry policy '+policy.hash:'No retry policy configured. Freezing a policy starts no workers.'));
 const freeze=el('button','Freeze retry policy');freeze.disabled=!!policy||['stopped','exhausted'].includes(search.state);freeze.addEventListener('click',async()=>{try{const delay=Number(backoff.value),maximum=Number(slots.value);if(!Number.isSafeInteger(delay)||delay<1||delay>86400000||!Number.isSafeInteger(maximum)||maximum<1||maximum>32)throw new Error('Retry values are outside supported bounds.');await retryAction(search,'retry-policy',{backoff_ms:delay,maximum_slots:maximum});}catch(error){failure(error);}});controls.append(freeze);
 const admit=el('button','Check retry admission');admit.disabled=!policy||search.state!=='paused'||search.control?.pause_origin!=='worker_error';admit.addEventListener('click',()=>retryAction(search,'retry-admit',{expected_epoch:search.control.epoch}).catch(failure));controls.append(admit);
 const history=el('button','View retry decision history');history.addEventListener('click',()=>{retryHistoryId=search.search_id;retryHistoryCursor=null;loadRetryHistory().catch(failure);});controls.append(history);
 const inspect=el('button','Inspect generated search');inspect.addEventListener('click',()=>loadGeneratedSearchDetail(search.search_id));controls.append(inspect);card.append(controls);host.append(card);}
 }catch(error){if(request===generatedSearchRequest){byId('generated-search-cards').replaceChildren();byId('generated-search-status').textContent='Generated search inventory unavailable; no verified state established.';failure(error);}}
}
let ownedListRequest=0,ownedRequest=0,ownedSelected='',ownedRaw='';
let ownedAfter=null,ownedNext=null,ownedStack=[];
function processHistoryButtons(prefix,after,stack,next,busy=false){
 byId(prefix+'-newer').disabled=busy||stack.length===0;
 byId(prefix+'-older').disabled=busy||next===null;
 byId(prefix+'-newest').disabled=busy||after===null;
}
async function loadOwned(after=ownedAfter,stack=ownedStack){
 const ticket=++ownedListRequest,requestedStack=[...stack];
 processHistoryButtons('owned',ownedAfter,ownedStack,ownedNext,true);
 byId('owned-cards').replaceChildren();byId('owned-status').textContent='Verifying process history page…';
 try{
  const data=await api('/api/owned-invocations?limit=32'+(after?'&after='+encodeURIComponent(after):'')).then(r=>r.json());
  if(ticket!==ownedListRequest)return;
  ownedAfter=after;ownedStack=requestedStack;ownedNext=data.next_cursor;
  const host=byId('owned-cards');host.replaceChildren();
  for(const row of data.rows){const card=el('article');card.className='card';card.dataset.invocationId=row.id;const inspect=el('button','Inspect process observation');inspect.addEventListener('click',()=>loadOwnedDetail(row.id));card.append(el('h3',row.id.slice(0,12)),el('p',row.observation+(row.summary?' · '+row.summary.stop_reason:'')),inspect);host.append(card);}
  byId('owned-status').textContent=data.rows.length+' parent invocations in this page · at most '+data.maximum_unclosed_owners+' unclosed owners; PID liveness is not established';
  processHistoryButtons('owned',ownedAfter,ownedStack,ownedNext);
 }catch(error){if(ticket===ownedListRequest){ownedNext=null;byId('owned-cards').replaceChildren();byId('owned-status').textContent='Verified process history unavailable.';processHistoryButtons('owned',ownedAfter,ownedStack,null);failure(error);}}
}
byId('owned-newest').addEventListener('click',()=>loadOwned(null,[]));
byId('owned-older').addEventListener('click',()=>{if(ownedNext)loadOwned(ownedNext,[...ownedStack,ownedAfter]);});
byId('owned-newer').addEventListener('click',()=>{if(ownedStack.length)loadOwned(ownedStack.at(-1),ownedStack.slice(0,-1));});
async function loadOwnedDetail(id){const ticket=++ownedRequest;ownedSelected=id;ownedRaw='';byId('owned-export').disabled=true;byId('owned-detail').hidden=false;byId('owned-json').textContent='Verifying process observation…';try{const raw=await api('/api/owned-invocations/'+id).then(r=>r.text());if(ticket!==ownedRequest)return;ownedRaw=raw;byId('owned-json').textContent=raw;byId('owned-export').disabled=false;}catch(error){if(ticket===ownedRequest){byId('owned-json').textContent='Verified process observation unavailable.';failure(error);}}}
byId('owned-refresh').addEventListener('click',()=>loadOwned());byId('owned-detail-refresh').addEventListener('click',()=>{if(ownedSelected)loadOwnedDetail(ownedSelected);});byId('owned-export').addEventListener('click',()=>{if(!ownedRaw)return;const link=el('a'),url=URL.createObjectURL(new Blob([ownedRaw],{type:'application/json'}));link.href=url;link.download='owned-process-'+ownedSelected+'.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});
let recoveryListRequest=0,recoveryRequest=0,recoverySelected='',recoveryRaw='',recoveryManifest='',recoveryFresh=false;
function recoveryPending(id){recoverySelected=id;recoveryRaw='';recoveryManifest='';recoveryFresh=false;byId('recovery-export').disabled=true;byId('recovery-verify').disabled=true;byId('recovery-restore').disabled=true;byId('recovery-detail').hidden=false;byId('recovery-json').textContent='Waiting for verified recovery receipt…';byId('recovery-detail-status').textContent=id+' · verification pending';}
function recoveryReceipt(raw){const v=JSON.parse(raw);recoverySelected=v.id;recoveryRaw=raw;recoveryManifest=v.manifest_hash;recoveryFresh=v.fresh_content_verified;byId('recovery-json').textContent=raw;byId('recovery-detail-status').textContent=v.id+' · '+(v.fresh_content_verified?'Actual contents verified':'Recorded manifest; verify actual contents before restore')+' · inactive; no service cutover';byId('recovery-export').disabled=false;byId('recovery-verify').disabled=false;byId('recovery-restore').disabled=!v.fresh_content_verified;}
async function loadRecovery(){const ticket=++recoveryListRequest;try{const data=await api('/api/recovery/snapshots').then(r=>r.json());if(ticket!==recoveryListRequest)return;const host=byId('recovery-cards');host.replaceChildren();for(const row of data.rows){const card=el('article');card.className='card';card.dataset.snapshotId=row.id;card.append(el('h3',row.id),el('p',row.status+(row.counts?' · completed records '+row.counts.completed:'')));const inspect=el('button','Inspect snapshot receipt');inspect.disabled=row.status!=='complete';inspect.addEventListener('click',()=>loadRecoveryDetail(row.id));card.append(inspect);host.append(card);}byId('recovery-status').textContent=data.rows.length+' retained entries · maximum '+data.maximum_snapshots+' · manifest inventory only; no automatic deletion';}catch(error){if(ticket===recoveryListRequest){byId('recovery-cards').replaceChildren();byId('recovery-status').textContent='Recovery inventory unavailable; preserve private evidence.';failure(error);}}}
async function loadRecoveryDetail(id){const ticket=++recoveryRequest;recoveryPending(id);try{const raw=await api('/api/recovery/snapshots/'+id).then(r=>r.text());if(ticket===recoveryRequest)recoveryReceipt(raw);}catch(error){if(ticket===recoveryRequest){byId('recovery-json').textContent='Verified snapshot receipt unavailable.';byId('recovery-detail-status').textContent='Unavailable';failure(error);}}}
async function recoveryAction(path,body,id){const ticket=++recoveryRequest;recoveryPending(id);try{const raw=await api(path,body).then(r=>r.text());if(ticket===recoveryRequest)recoveryReceipt(raw);await loadRecovery();}catch(error){if(ticket===recoveryRequest){byId('recovery-json').textContent='Recovery action refused. Existing paths and evidence are preserved.';byId('recovery-detail-status').textContent='Refused; inspect retained evidence';failure(error);}await loadRecovery();}}
byId('recovery-create-form').addEventListener('submit',async e=>{e.preventDefault();const id=byId('recovery-new-id').value;byId('recovery-create').disabled=true;try{await recoveryAction('/api/recovery/snapshots',{id},id);}finally{byId('recovery-create').disabled=false;}});
byId('recovery-verify').addEventListener('click',()=>{if(recoverySelected)recoveryAction('/api/recovery/snapshots/'+recoverySelected+'/verify',{},recoverySelected);});
byId('recovery-restore-form').addEventListener('submit',e=>{e.preventDefault();if(!recoverySelected||!recoveryManifest||!recoveryFresh)return;const id=byId('recovery-restore-id').value;recoveryAction('/api/recovery/snapshots/'+recoverySelected+'/restore',{id,expected_manifest_hash:recoveryManifest},id);});
byId('recovery-refresh').addEventListener('click',loadRecovery);byId('recovery-export').addEventListener('click',()=>{if(!recoveryRaw)return;const link=el('a'),url=URL.createObjectURL(new Blob([recoveryRaw],{type:'application/json'}));link.href=url;link.download='snapshot-'+recoverySelected+'.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});
let supervisionListRequest=0,supervisionRequest=0,supervisionSelected='',supervisionRaw='';
let supervisionAfter=null,supervisionNext=null,supervisionStack=[];
async function loadSupervision(after=supervisionAfter,stack=supervisionStack){
 const ticket=++supervisionListRequest,requestedStack=[...stack];
 processHistoryButtons('supervision',supervisionAfter,supervisionStack,supervisionNext,true);
 byId('supervision-cards').replaceChildren();byId('supervision-status').textContent='Verifying session history page…';
 try{
  const data=await api('/api/supervision-sessions?limit=32'+(after?'&after='+encodeURIComponent(after):'')).then(r=>r.json());
  if(ticket!==supervisionListRequest)return;
  supervisionAfter=after;supervisionStack=requestedStack;supervisionNext=data.next_cursor;
  const host=byId('supervision-cards');host.replaceChildren();
  for(const row of data.rows){const card=el('article');card.className='card';card.dataset.sessionId=row.id;const inspect=el('button','Inspect session');inspect.addEventListener('click',()=>loadSupervisionDetail(row.id));card.append(el('h3',row.id.slice(0,12)),el('p',row.observation+' · '+row.round_attempts+' rounds · '+row.checks+' checks'),inspect);host.append(card);}
  byId('supervision-status').textContent=data.rows.length+' sessions in this page · at most '+data.maximum_unclosed_owners+' unclosed owners; PID liveness is not established';
  processHistoryButtons('supervision',supervisionAfter,supervisionStack,supervisionNext);
 }catch(error){if(ticket===supervisionListRequest){supervisionNext=null;byId('supervision-cards').replaceChildren();byId('supervision-status').textContent='Verified session inventory unavailable.';processHistoryButtons('supervision',supervisionAfter,supervisionStack,null);failure(error);}}
}
byId('supervision-newest').addEventListener('click',()=>loadSupervision(null,[]));
byId('supervision-older').addEventListener('click',()=>{if(supervisionNext)loadSupervision(supervisionNext,[...supervisionStack,supervisionAfter]);});
byId('supervision-newer').addEventListener('click',()=>{if(supervisionStack.length)loadSupervision(supervisionStack.at(-1),supervisionStack.slice(0,-1));});
async function loadSupervisionDetail(id){const ticket=++supervisionRequest;supervisionSelected=id;supervisionRaw='';byId('supervision-export').disabled=true;byId('supervision-stop').disabled=true;byId('supervision-detail').hidden=false;byId('supervision-json').textContent='Verifying session snapshot…';try{const raw=await api('/api/supervision-sessions/'+id).then(r=>r.text());if(ticket!==supervisionRequest)return;const data=JSON.parse(raw);supervisionRaw=raw;byId('supervision-json').textContent=raw;byId('supervision-detail-status').textContent=data.observation+(data.stop_requested_at_ms?' · stop requested':'');byId('supervision-export').disabled=false;byId('supervision-stop').disabled=data.finished_at_ms!==null||data.stop_requested_at_ms!==null;}catch(error){if(ticket===supervisionRequest){byId('supervision-json').textContent='Verified session snapshot unavailable.';byId('supervision-detail-status').textContent='Unavailable';failure(error);}}}
byId('supervision-refresh').addEventListener('click',()=>loadSupervision());
byId('supervision-detail-refresh').addEventListener('click',()=>{if(supervisionSelected)loadSupervisionDetail(supervisionSelected);});
byId('supervision-stop').addEventListener('click',async()=>{const id=supervisionSelected,ticket=++supervisionRequest;supervisionRaw='';byId('supervision-stop').disabled=true;byId('supervision-export').disabled=true;try{await api('/api/supervision-sessions/'+id+'/stop',{});if(ticket===supervisionRequest)await loadSupervisionDetail(id);await loadSupervision();}catch(error){if(ticket===supervisionRequest){await loadSupervisionDetail(id);failure(error);}}});
byId('supervision-export').addEventListener('click',()=>{if(!supervisionRaw)return;const link=el('a'),url=URL.createObjectURL(new Blob([supervisionRaw],{type:'application/json'}));link.href=url;link.download='supervision-'+supervisionSelected+'.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});
let retryHistoryId='',retryHistoryCursor=null,retryHistoryNext=null,retryHistoryRaw='',retryHistoryRequest=0;
async function loadRetryHistory(){const request=++retryHistoryRequest,id=retryHistoryId;retryHistoryRaw='';byId('retry-history-export').disabled=true;byId('retry-history-next').disabled=true;try{const raw=await api('/api/corpus-searches/'+id+'/retry-decisions?limit=16'+(retryHistoryCursor?'&after='+encodeURIComponent(retryHistoryCursor):'')).then(r=>r.text());if(request!==retryHistoryRequest||id!==retryHistoryId)return;const data=JSON.parse(raw);retryHistoryRaw=raw;retryHistoryNext=data.next_cursor;byId('retry-history-json').textContent=raw;byId('retry-history-status').textContent=id+' · '+data.rows.length+' decisions in this page · '+data.total+' recorded';byId('retry-history-first').disabled=retryHistoryCursor===null;byId('retry-history-next').disabled=retryHistoryNext===null;byId('retry-history-export').disabled=false;}catch(error){if(request===retryHistoryRequest){byId('retry-history-json').textContent='Verified decision history unavailable.';retryHistoryNext=null;byId('retry-history-next').disabled=true;}throw error;}}
byId('retry-history-first').addEventListener('click',()=>{retryHistoryCursor=null;loadRetryHistory().catch(failure);});byId('retry-history-next').addEventListener('click',()=>{if(retryHistoryNext){retryHistoryCursor=retryHistoryNext;loadRetryHistory().catch(failure);}});byId('retry-history-export').addEventListener('click',()=>{if(!retryHistoryRaw)return;const link=el('a'),url=URL.createObjectURL(new Blob([retryHistoryRaw],{type:'application/json'}));link.href=url;link.download='retry-history-'+retryHistoryId+'.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});
async function retryAction(search,action,body){const request=++retryRequest;retryRaw='';byId('retry-export').disabled=true;try{const raw=await api('/api/corpus-searches/'+search.search_id+'/'+action,body).then(r=>r.text());if(request!==retryRequest)return;const value=JSON.parse(raw);retryRaw=raw;byId('retry-export').disabled=false;byId('retry-action').textContent=search.search_id.slice(0,12)+' · '+(value.kind??'policy configured')+(value.reason?' · '+value.reason:'')+(value.retry_at_ms?' · retry at '+value.retry_at_ms:'')+' · no workers started';await loadGeneratedSearches();if(request===retryRequest&&generatedSelected===search.search_id)await loadGeneratedSearchDetail(search.search_id);}catch(error){if(request===retryRequest){retryRaw='';byId('retry-export').disabled=true;byId('retry-action').textContent='Retry action unavailable.';}throw error;}}

byId('retry-export').addEventListener('click',()=>{if(!retryRaw)return;const link=el('a'),url=URL.createObjectURL(new Blob([retryRaw],{type:'application/json'}));link.href=url;link.download='retry-decision.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});
async function loadGeneratedSearchDetail(id){const request=++generatedDetailRequest;generatedSelected=id;generatedRaw='';byId('generated-search-export').disabled=true;byId('generated-search-detail').hidden=false;byId('generated-search-json').textContent='Verifying generated search snapshot…';
 try{const raw=await api('/api/corpus-searches/'+id).then(r=>r.text());if(request!==generatedDetailRequest)return;generatedRaw=raw;byId('generated-search-json').textContent=raw;byId('generated-search-export').disabled=false;}catch(error){if(request===generatedDetailRequest){byId('generated-search-json').textContent='Verified generated search snapshot unavailable.';failure(error);}}
}
byId('generated-search-refresh').addEventListener('click',loadGeneratedSearches);
byId('generated-search-detail-refresh').addEventListener('click',()=>{if(generatedSelected)loadGeneratedSearchDetail(generatedSelected);});
byId('generated-search-export').addEventListener('click',()=>{if(!generatedRaw)return;const link=el('a'),url=URL.createObjectURL(new Blob([generatedRaw],{type:'application/json'}));link.href=url;link.download='corpus-search-'+generatedSelected+'.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});
let rotationRequest=0,rotationDetailRequest=0,rotationSelected=null,rotationRaw='';
function clearRotation(message){rotationRaw='';byId('rotation-cost-chart').replaceChildren();byId('rotation-export').disabled=true;byId('rotation-controls').replaceChildren();byId('rotation-json').textContent=message;}
async function loadRotations(){const ticket=++rotationRequest;try{const rows=await api('/api/campaign-rotations').then(r=>r.json());if(ticket!==rotationRequest)return;const host=byId('rotation-cards');host.replaceChildren();byId('rotation-status').textContent=rows.length+' frozen rotations. Observed costs cover linked parents; unknown outcomes are separate. Stored metadata does not verify current input eligibility.';for(const row of rows){const card=el('article');card.className='card';card.dataset.rotationId=row.rotation_id;card.append(el('h3','Rotation '+row.rotation_id.slice(0,12)),el('p',row.state+' · next member '+row.next_member_index+' · '+row.visits+' published visits'),el('p',row.members.length+' frozen members · '+(row.pause_origin||'no pause')),el('p','Synthetic training only · qualification withheld'));for(const member of row.members){card.append(el('p',member.name+' · '+member.last_kind+' · '+member.known_children+' known children · '+member.unknown_outcome_visits+' unknown outcome visits'),el('p','Observed CPU '+member.observed_native_cpu_ticks+' native ticks ('+member.cpu_observed_children+' measured, '+member.cpu_unobserved_children+' unmeasured children) · observed parent elapsed '+member.observed_parent_elapsed_ms+' ms'),el('p',member.current_metadata_available?'Stored member state: '+member.current_state:'Member metadata unavailable'));}const inspect=el('button','Inspect rotation');inspect.addEventListener('click',()=>loadRotationDetail(row.rotation_id));card.append(inspect);host.append(card);}}catch(error){if(ticket===rotationRequest){byId('rotation-cards').replaceChildren();byId('rotation-status').textContent='Rotation inventory unavailable.';++rotationDetailRequest;clearRotation('Verified rotation snapshot unavailable.');failure(error);}}}
async function loadRotationDetail(id){const ticket=++rotationDetailRequest;rotationSelected=id;byId('rotation-detail').hidden=false;clearRotation('Verifying rotation scope and progress…');try{const raw=await api('/api/campaign-rotations/'+id).then(r=>r.text());if(ticket!==rotationDetailRequest||id!==rotationSelected)return;const snapshot=JSON.parse(raw);rotationRaw=raw;byId('rotation-json').textContent=raw;renderRotationCosts(snapshot);byId('rotation-export').disabled=false;for(const[label,state]of[['Pause rotation','paused'],['Resume rotation','running'],['Stop rotation permanently','stopped']]){const button=el('button',label);const current=snapshot.state;button.disabled=['stopped','exhausted'].includes(current)||(state==='paused'&&current!=='running'&&!(current==='paused'&&snapshot.pause_origin!=='operator'))||(state==='running'&&current!=='paused');button.addEventListener('click',()=>changeRotation(id,state,snapshot.epoch));byId('rotation-controls').append(button);}}catch(error){if(ticket===rotationDetailRequest&&id===rotationSelected){clearRotation('Verified rotation snapshot unavailable.');failure(error);}}}
async function changeRotation(id,state,epoch){if(id!==rotationSelected||!rotationRaw)return;const ticket=++rotationDetailRequest;clearRotation('Applying rotation control…');try{await api('/api/campaign-rotations/'+id+'/control',{state,expected_epoch:epoch});if(ticket!==rotationDetailRequest||id!==rotationSelected)return;await loadRotationDetail(id);await loadRotations();}catch(error){if(ticket===rotationDetailRequest&&id===rotationSelected){clearRotation('Rotation control refused; refresh the snapshot before trying again.');failure(error);}}}
byId('rotation-refresh').addEventListener('click',loadRotations);byId('rotation-detail-refresh').addEventListener('click',()=>{if(rotationSelected)loadRotationDetail(rotationSelected);});byId('rotation-export').addEventListener('click',()=>{if(!rotationRaw)return;const link=el('a'),url=URL.createObjectURL(new Blob([rotationRaw],{type:'application/json'}));link.href=url;link.download='corpus-rotation-'+rotationSelected+'.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});

let campaignRequest=0,campaignDetailRequest=0,campaignSelected=null,campaignRaw='';
function clearCampaign(message){clearCampaignLedger();clearCampaignVerify();campaignRaw='';byId('campaign-export').disabled=true;byId('campaign-controls').replaceChildren();byId('campaign-json').textContent=message;}
async function loadCampaigns(){const ticket=++campaignRequest;try{const rows=await api('/api/campaigns').then(r=>r.json());if(ticket!==campaignRequest)return;const host=byId('campaign-cards');host.replaceChildren();byId('campaign-status').textContent=rows.length+' frozen campaigns. Metadata inspection does not verify current source eligibility.';for(const row of rows){const card=el('article');card.className='card';card.dataset.campaignId=row.campaign_id;card.append(el('h3','Campaign '+row.campaign_id.slice(0,12)),el('p',row.execution.state+' · '+row.execution.next_region+' of '+row.regions.length+' regions complete'),el('p',row.declared_slots+' declared slots · '+row.distinct_experiments+' distinct experiment identities'),el('p','Synthetic training only · qualification withheld'));const inspect=el('button','Inspect campaign');inspect.addEventListener('click',()=>loadCampaignDetail(row.campaign_id));card.append(inspect);host.append(card);}}catch(error){if(ticket===campaignRequest){byId('campaign-cards').replaceChildren();byId('campaign-status').textContent='Campaign inventory unavailable.';++campaignDetailRequest;clearCampaign('Verified campaign snapshot unavailable.');failure(error);}}}
async function loadCampaignDetail(id){const ticket=++campaignDetailRequest;campaignSelected=id;byId('campaign-detail').hidden=false;clearCampaign('Verifying campaign scope and progress…');try{const raw=await api('/api/campaigns/'+id).then(r=>r.text());if(ticket!==campaignDetailRequest||id!==campaignSelected)return;const snapshot=JSON.parse(raw);campaignRaw=raw;byId('campaign-owned-open').disabled=false;byId('campaign-verify-inputs').disabled=false;byId('campaign-json').textContent=raw;byId('campaign-export').disabled=false;for(const[label,state]of[['Pause campaign','paused'],['Resume campaign','running'],['Stop campaign permanently','stopped']]){const button=el('button',label);const current=snapshot.execution.state;button.disabled=['stopped','exhausted'].includes(current)||(state==='paused'&&current!=='running')||(state==='running'&&current!=='paused');button.addEventListener('click',()=>changeCampaign(id,state,snapshot.execution.epoch));byId('campaign-controls').append(button);}}catch(error){if(ticket===campaignDetailRequest&&id===campaignSelected){clearCampaign('Verified campaign snapshot unavailable.');failure(error);}}}
let campaignOwnedRequest=0,campaignOwnedId=null,campaignOwnedAfter=null,campaignOwnedNext=null,campaignOwnedStack=[],campaignOwnedRaw='';
function clearCampaignLedger(){++campaignOwnedRequest;campaignOwnedRaw='';byId('campaign-owned-export').disabled=true;byId('campaign-owned-open').disabled=true;byId('campaign-owned-detail').hidden=true;}
async function loadCampaignOwned(after=campaignOwnedAfter,stack=campaignOwnedStack){
 const id=campaignSelected;if(!id||!campaignRaw)return;
 if(campaignOwnedId!==id){campaignOwnedId=id;campaignOwnedAfter=null;campaignOwnedNext=null;campaignOwnedStack=[];after=null;stack=[];}
 const ticket=++campaignOwnedRequest,requestedStack=[...stack];campaignOwnedRaw='';byId('campaign-owned-detail').hidden=false;byId('campaign-owned-export').disabled=true;processHistoryButtons('campaign-owned',campaignOwnedAfter,campaignOwnedStack,campaignOwnedNext,true);byId('campaign-owned-json').textContent='Verifying complete retained chain and this page…';
 try{const raw=await api('/api/campaigns/'+id+'/owned-visits?limit=32'+(after?'&after='+encodeURIComponent(after):'')).then(r=>r.text());if(ticket!==campaignOwnedRequest||id!==campaignSelected)return;const data=JSON.parse(raw);campaignOwnedAfter=after;campaignOwnedStack=requestedStack;campaignOwnedNext=data.next_cursor;campaignOwnedRaw=raw;byId('campaign-owned-json').textContent=raw;byId('campaign-owned-status').textContent=data.rows.length+' visits in this page · '+data.total+' verified retained · frozen budget '+data.owned_visit_limit+' · cost counters distinguish observed and unknown';byId('campaign-owned-export').disabled=false;processHistoryButtons('campaign-owned',after,campaignOwnedStack,campaignOwnedNext);}
 catch(error){if(ticket===campaignOwnedRequest&&id===campaignSelected){campaignOwnedNext=null;byId('campaign-owned-json').textContent='Verified campaign ledger unavailable.';byId('campaign-owned-status').textContent='Unavailable';processHistoryButtons('campaign-owned',campaignOwnedAfter,campaignOwnedStack,null);failure(error);}}
}
byId('campaign-owned-open').addEventListener('click',()=>loadCampaignOwned());
byId('campaign-owned-refresh').addEventListener('click',()=>loadCampaignOwned());
byId('campaign-owned-older').addEventListener('click',()=>{if(campaignOwnedNext)loadCampaignOwned(campaignOwnedNext,[...campaignOwnedStack,campaignOwnedAfter]);});
byId('campaign-owned-newer').addEventListener('click',()=>{if(campaignOwnedStack.length)loadCampaignOwned(campaignOwnedStack.at(-1),campaignOwnedStack.slice(0,-1));});
byId('campaign-owned-newest').addEventListener('click',()=>loadCampaignOwned(null,[]));
byId('campaign-owned-export').addEventListener('click',()=>{if(!campaignOwnedRaw||campaignOwnedId!==campaignSelected)return;const link=el('a'),url=URL.createObjectURL(new Blob([campaignOwnedRaw],{type:'application/json'}));link.href=url;link.download='campaign-'+campaignOwnedId+'-visits.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});
async function changeCampaign(id,state,epoch){if(id!==campaignSelected||!campaignRaw)return;const ticket=++campaignDetailRequest;clearCampaign('Applying campaign control…');try{await api('/api/campaigns/'+id+'/control',{state,expected_epoch:epoch});if(ticket!==campaignDetailRequest||id!==campaignSelected)return;await loadCampaignDetail(id);await loadCampaigns();}catch(error){if(ticket===campaignDetailRequest&&id===campaignSelected){clearCampaign('Campaign control refused; refresh the snapshot before trying again.');failure(error);}}}
byId('campaign-refresh').addEventListener('click',loadCampaigns);byId('campaign-detail-refresh').addEventListener('click',()=>{if(campaignSelected)loadCampaignDetail(campaignSelected);});byId('campaign-export').addEventListener('click',()=>{if(!campaignRaw)return;const link=el('a'),url=URL.createObjectURL(new Blob([campaignRaw],{type:'application/json'}));link.href=url;link.download='corpus-campaign-'+campaignSelected+'.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});

let fleetRequest=0,fleetDetailRequest=0,fleetSelected='',fleetRaw='';
async function loadFleets(){const request=++fleetRequest;try{const rows=await api('/api/corpus-fleets').then(r=>r.json());if(request!==fleetRequest)return;const host=byId('fleet-cards');host.replaceChildren();byId('fleet-status').textContent=rows.length+' registered input rotations. Visits and progress visits are not new simulations.';
 for(const fleet of rows){const card=el('article');card.className='card';card.dataset.fleetId=fleet.fleet_id;card.append(el('h3',fleet.fleet_id.slice(0,12)),el('p',fleet.state+' · visits '+fleet.visits+' · next input '+fleet.next_source_index),el('p','pause origin '+(fleet.control?.pause_origin??'none')+' · epoch '+(fleet.control?.epoch??'unknown')),el('p',fleet.lease_active?'Rotation coordinator lease active':'No active rotation coordinator lease'),el('p',fleet.disk_reserve_bytes?'Disk reserve '+fleet.disk_reserve_bytes+' bytes':'No fleet disk reserve configured'));
 for(const cost of fleet.owned_observations??[]){card.append(el('p','Input '+cost.source_index+' · owned visits '+cost.owned_visits+' · observed CPU '+cost.observed_cpu_ticks+' native ticks · unobserved visits '+cost.cpu_unobserved_visits+' · parent elapsed '+cost.observed_parent_elapsed_ms+' ms'));}
 for(const source of fleet.sources){card.append(el('p',source.index+' · '+source.name+' · last visit '+source.last_kind+' · '+source.visits+' visits'));if(source.last_obstacle)card.append(el('p','Last returned obstacle at source visit '+source.obstacle_at_visit+': '+source.last_obstacle));}
 const controls=el('div');controls.className='controls';for(const [label,state]of [['Pause input rotation','paused'],['Resume input rotation','running'],['Stop input rotation permanently','stopped']]){const button=el('button',label);button.disabled=['stopped','exhausted'].includes(fleet.state)||(state==='paused'&&fleet.state!=='running'&&!(fleet.state==='paused'&&fleet.control?.pause_origin!=='operator'))||(state==='running'&&fleet.state!=='paused');button.addEventListener('click',async()=>{button.disabled=true;try{await api('/api/corpus-fleets/'+fleet.fleet_id+'/control',{state});await loadFleets();if(fleetSelected===fleet.fleet_id)await loadFleetDetail(fleet.fleet_id);}catch(error){await loadFleets();failure(error);}});controls.append(button);}const inspect=el('button','Inspect input rotation');inspect.addEventListener('click',()=>loadFleetDetail(fleet.fleet_id));controls.append(inspect);card.append(controls);host.append(card);}
 }catch(error){if(request===fleetRequest){byId('fleet-cards').replaceChildren();byId('fleet-status').textContent='Verified input rotation inventory unavailable.';failure(error);}}
}
async function loadFleetDetail(id){const request=++fleetDetailRequest;fleetSelected=id;fleetRaw='';byId('fleet-cost-chart').replaceChildren();byId('fleet-export').disabled=true;byId('fleet-detail').hidden=false;byId('fleet-json').textContent='Verifying input rotation snapshot…';try{const raw=await api('/api/corpus-fleets/'+id).then(r=>r.text());if(request!==fleetDetailRequest)return;fleetRaw=raw;byId('fleet-json').textContent=raw;renderFleetCosts(JSON.parse(raw));byId('fleet-export').disabled=false;}catch(error){if(request===fleetDetailRequest){byId('fleet-json').textContent='Verified input rotation snapshot unavailable.';byId('fleet-cost-chart').replaceChildren();failure(error);}}}
byId('fleet-refresh').addEventListener('click',loadFleets);byId('fleet-detail-refresh').addEventListener('click',()=>{if(fleetSelected)loadFleetDetail(fleetSelected);});byId('fleet-export').addEventListener('click',()=>{if(!fleetRaw)return;const link=el('a'),url=URL.createObjectURL(new Blob([fleetRaw],{type:'application/json'}));link.href=url;link.download='corpus-fleet-'+fleetSelected+'.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});
async function loadResources(){
 try {const data=await api('/api/resources').then(r=>r.json());const panel=byId('resources');panel.replaceChildren();for(const [label,value]of [['Dashboard service RSS',data.process_rss_bytes+' bytes'],['Host available memory',data.host_available_memory_bytes+' bytes'],['Available parallelism',data.available_parallelism],['Dashboard peak RSS (high-water)',data.peak_rss_bytes+' bytes'],['Dashboard CPU ticks (user + system)',(BigInt(data.process_user_ticks)+BigInt(data.process_system_ticks)).toString()+' ticks'],['Dashboard start ticks',data.process_start_ticks+' ticks']]){const card=el('article');card.className='card';card.append(el('h3',label),el('p',value));panel.append(card);}byId('resource-status').textContent='Current dashboard process and host observations. '+data.cpu_scope+' Standalone research processes have separate memory use; these observations do not enforce OS limits.';}
 catch(_error){byId('resources').replaceChildren();byId('resource-status').textContent='Runtime resource observations unavailable.';}
}
async function loadArtifacts(){
 const panel=byId('artifacts');try{const data=await api('/api/artifacts').then(r=>r.json());panel.replaceChildren();for(const [label,value]of [['File-backed receipts',data.referenced_receipts],['Stored file-length bytes',data.physical_bytes],['Uncommitted partial files',data.partial_files],['Store byte cap',data.maximum_store_bytes]]){const card=el('article');card.className='card';card.append(el('h3',label),el('p',value));panel.append(card);}byId('artifact-status').textContent=data.limitations;}catch{panel.replaceChildren();byId('artifact-status').textContent='Artifact inventory unavailable. Preserve evidence and inspect the registry.';}
}
async function loadDisk(){
 const panel=byId('disk');try{const data=await api('/api/disk').then(r=>r.json());panel.replaceChildren();for(const [label,value]of [['Available to service user',data.available_bytes+' bytes'],['Filesystem capacity',data.total_bytes+' bytes']]){const card=el('article');card.className='card';card.append(el('h3',label),el('p',value));panel.append(card);}byId('disk-status').textContent=data.scope;}catch{panel.replaceChildren();byId('disk-status').textContent='Filesystem observations unavailable; no zero capacity is inferred.';}
}
async function refresh(){const [overview]=await Promise.all([api("/api/overview?runs=false").then(r=>r.json()),protocolPager.load(),runPager.load(),loadHistory(),loadCatalog(),loadResources(),loadRecovery(),loadOwned(),loadSupervision(),loadGeneratedSearches(),loadCampaigns(),loadRotations(),loadFleets(),loadArtifacts(),loadDisk(),loadCoverageTargets()]);renderAuditHealth(overview.audit_health);currentEngine=overview.engine;byId('readiness').textContent=overview.software_status+' · '+overview.historical_readiness;byId('qualification').textContent=overview.qualification;byId('metrics').replaceChildren();for(const [label,value]of Object.entries(overview.counts)){const card=el('div');card.className='card';card.append(el('span',({registered:'Registered experiments',completed:'Committed tick results',running:'Running experiment records',attempts:'Lease acquisitions'})[label]||label),Object.assign(el('strong',value),{className:'metric'}));byId('metrics').append(card);}renderResults();}
function renderDatasets(){const seen=new Set();byId('datasets').replaceChildren();for(const row of rows){const m=row.manifest;if(!m)continue;const key=JSON.stringify([m.content_hash,m.profile_hash,m.archive_hash,m.class,m.source,m.parser_version]);if(seen.has(key))continue;seen.add(key);const card=el('article');card.className='card';card.append(el('h3',m.symbol),el('p',m.class),el('p',m.events+' quote events'),el('p',new Date(m.first_ms).toISOString()+' → '+new Date(m.last_ms).toISOString()),el('p',m.source),el('p','Content '+m.content_hash));byId('datasets').append(card);}if(!seen.size)byId('datasets').append(el('p','No provenance manifests in this experiment page.'));}
(async()=>{csrf=(await api('/api/session').then(r=>r.json())).csrf;await refresh();await loadImports();setInterval(()=>refresh().catch(failure),10000);})().catch(failure);

let reviewRequest=0,reviewAfter=null,reviewNext=null,reviewHead=null,reviewRaw='',reviewIdentity='';
function clearLiveReview(){byId('review-live').hidden=true;byId('review-live').textContent='';byId('review-live-status').textContent='Current artifacts have not been revalidated in this view.';}
async function loadDataReview(id,after=null){
 const request=++reviewRequest;reviewIdentity=id;reviewAfter=after;reviewRaw='';reviewNext=null;clearLiveReview();byId('review-form').hidden=true;byId('review-export').disabled=true;byId('review-older').disabled=true;byId('review-history').textContent='';byId('data-review').hidden=false;byId('data-review-status').textContent='Loading independent review history…';
 try{
  const query=new URLSearchParams({limit:'25'});if(after!==null)query.set('before',after);
  const raw=await api('/api/data-review/'+id+'?'+query).then(r=>r.text());if(request!==reviewRequest||catalogSelected!==id)return;
  const data=JSON.parse(raw);reviewRaw=raw;reviewHead=data.head??null;reviewNext=data.next_cursor??null;byId('review-older').disabled=reviewNext===null;byId('review-export').disabled=false;
  byId('data-review-status').textContent=data.configured?'Showing '+data.events.length+' independent decisions. '+(data.current_authority_authenticated?'Stored decisions authenticate under current authority. ':'Some stored decisions are expired or do not authenticate under the current authority. ')+data.limitations:data.message;
  byId('review-history').textContent=diagnosticJson(data);
  const select=byId('review-machine'),previous=select.value;select.replaceChildren();for(const a of data.admissions??[]){const option=el('option',a.id.slice(0,12)+' · '+a.scope.start_ms+'…'+a.scope.end_exclusive_ms);option.value=a.id;select.append(option);}if([...select.options].some(o=>o.value===previous))select.value=previous;
  byId('review-form').hidden=!(data.configured&&select.options.length);byId('review-submit').disabled=after!==null;
 }catch(error){if(request!==reviewRequest||catalogSelected!==id)return;reviewRaw='';reviewHead=null;byId('data-review-status').textContent='Independent review unavailable; no approval or current eligibility established.';failure(error);}
}
byId('review-latest').addEventListener('click',()=>{if(catalogSelected)loadDataReview(catalogSelected);});
byId('review-older').addEventListener('click',()=>{if(reviewNext!==null)loadDataReview(catalogSelected,reviewNext);});
byId('review-machine').addEventListener('change',clearLiveReview);
byId('review-export').addEventListener('click',()=>{if(!reviewRaw)return;const link=el('a'),url=URL.createObjectURL(new Blob([reviewRaw],{type:'application/json'}));link.href=url;link.download='data-review-'+reviewIdentity+'.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});
byId('review-revalidate').addEventListener('click',async()=>{
 const id=catalogSelected,machine=byId('review-machine').value,request=reviewRequest;clearLiveReview();byId('review-revalidate').disabled=true;byId('review-live-status').textContent='Revalidating exact configured artifacts…';
 try{const raw=await api('/api/data-eligibility/'+machine).then(r=>r.text());if(id!==catalogSelected||request!==reviewRequest||machine!==byId('review-machine').value)return;const data=JSON.parse(raw);byId('review-live-status').textContent=data.state+' · '+(data.live_machine_revalidated?'Exact current artifacts revalidated. ':'Current artifact verification failed. ')+(data.reasons??[]).join('; ');byId('review-live').textContent=diagnosticJson(data);byId('review-live').hidden=false;}catch(error){if(id===catalogSelected&&request===reviewRequest){clearLiveReview();byId('review-live-status').textContent='Current artifact verification unavailable; no eligibility established.';failure(error);}}finally{byId('review-revalidate').disabled=false;}
});
byId('review-form').addEventListener('submit',async event=>{
 event.preventDefault();const id=catalogSelected,head=reviewHead,machine=byId('review-machine').value;
 const body={reviewer:byId('reviewer-name').value,password:byId('reviewer-password').value,request:{machine_id:machine,expected_head:head,area:byId('review-area').value,decision:byId('review-decision').value,explanation:byId('review-explanation').value,evidence_text:byId('review-evidence').value}};
 byId('reviewer-password').value='';byId('review-submit').disabled=true;clearLiveReview();
 try{await api('/api/data-review',body);if(id===catalogSelected){await loadDataReview(id);byId('data-review-status').textContent='Independent decision recorded. '+byId('data-review-status').textContent;}}catch(error){if(id===catalogSelected){await loadDataReview(id);failure(error);}}finally{if(id===catalogSelected)byId('review-submit').disabled=reviewAfter!==null;}
});


let importsRequest=0,importsRaw='',importDetailRequest=0,importSelected='',importRaw='';
async function loadImports(){
 const ticket=++importsRequest;importsRaw='';byId('imports-export').disabled=true;
 byId('imports-status').textContent='Verifying import checkpoints…';
 try{
  const raw=await api('/api/imports').then(r=>r.text());if(ticket!==importsRequest)return;
  const data=JSON.parse(raw);importsRaw=raw;byId('imports-export').disabled=false;
  const host=byId('import-cards');host.replaceChildren();
  if(!data.configured){byId('imports-status').textContent=data.limitations;return;}
  byId('imports-status').textContent=data.imports.length+' configured import directories · at most '+data.maximum_imports+'. '+data.limitations;
  for(const row of data.imports){const card=el('article');card.className='card';card.dataset.importName=row.name;card.append(el('h3',row.name),el('p','State: '+row.state));
   if(row.state==='complete'||row.state==='in_progress')card.append(el('p','Verified events '+row.events+' · chunks '+row.chunks),el('p','Source archive '+row.archive_hash.slice(0,12)+' · '+row.archive_bytes+' bytes · '+(row.synthetic?'synthetic declared':'real declared')),el('p','Started '+row.started_at));
   if(row.state==='refused')card.append(el('p','Verification refused: '+row.reason));
   if(row.state==='not_initialized')card.append(el('p',row.limitations));
   if(row.decision_history==='corrupt')card.append(el('p','Decision history corrupt: '+row.decision_history_reason));
   else if(row.last_decision)card.append(el('p','Last decision: '+row.last_decision.kind+' · '+row.decision_count+' retained'));
   const inspect=el('button','Inspect import receipt');inspect.disabled=row.state==='not_initialized';inspect.addEventListener('click',()=>loadImportDetail(row.name).catch(failure));card.append(inspect);host.append(card);}
 }catch(error){if(ticket===importsRequest){byId('import-cards').replaceChildren();byId('imports-status').textContent='Import inventory unavailable; no verified state established.';failure(error);}}
}
async function loadImportDetail(name){
 const ticket=++importDetailRequest;importSelected=name;importRaw='';
 byId('import-detail').hidden=false;byId('import-export').disabled=true;byId('import-activate-form').hidden=true;byId('import-refuse-form').hidden=true;
 byId('import-json').textContent='Verifying import receipt…';byId('import-detail-label').textContent=name;byId('import-decision-status').textContent='';
 try{
  const raw=await api('/api/imports/'+encodeURIComponent(name)).then(r=>r.text());
  if(ticket!==importDetailRequest||name!==importSelected)return;
  const data=JSON.parse(raw);importRaw=raw;byId('import-json').textContent=raw;byId('import-export').disabled=false;
  byId('import-detail-label').textContent=name+' · '+data.state+' · every published chunk re-verified; declared class is not approval';
  const p=data.progress;
  byId('import-activate-hash').value=p.configuration.archive_hash;byId('import-activate-events').value=String(p.events);byId('import-refuse-hash').value=p.configuration.archive_hash;
  byId('import-activate-form').hidden=data.state!=='complete';byId('import-refuse-form').hidden=false;
  byId('import-decision-status').textContent=data.decisions.length+' retained decisions · activation registers a catalog observation only';
 }catch(error){if(ticket===importDetailRequest&&name===importSelected){byId('import-json').textContent='Verified import receipt unavailable.';byId('import-detail-label').textContent=name+' · verification refused';byId('import-refuse-hash').value='';byId('import-refuse-form').hidden=false;failure(error);}}
}
async function importAction(path,body){
 const name=importSelected;if(!name)return;
 const ticket=++importDetailRequest;
 byId('import-activate-submit').disabled=true;byId('import-refuse-submit').disabled=true;
 byId('import-decision-status').textContent='Applying operator decision…';
 try{
  const raw=await api(path,body).then(r=>r.text());
  if(ticket!==importDetailRequest||name!==importSelected)return;
  const kind=JSON.parse(raw).decision.kind;
  await loadImportDetail(name);await loadImports();
  if(importSelected===name)byId('import-decision-status').textContent='Decision recorded: '+kind+' · no approval, eligibility or qualification granted';
 }catch(error){
  if(ticket===importDetailRequest&&name===importSelected){failure(error);await loadImportDetail(name).catch(()=>{});if(importSelected===name)byId('import-decision-status').textContent='Decision refused; existing artifacts and evidence are preserved.';}
 }finally{byId('import-activate-submit').disabled=false;byId('import-refuse-submit').disabled=false;}
}
byId('imports-refresh').addEventListener('click',loadImports);
byId('imports-export').addEventListener('click',()=>{if(!importsRaw)return;const link=el('a'),url=URL.createObjectURL(new Blob([importsRaw],{type:'application/json'}));link.href=url;link.download='import-inventory.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});
byId('import-detail-refresh').addEventListener('click',()=>{if(importSelected)loadImportDetail(importSelected).catch(failure);});
byId('import-export').addEventListener('click',()=>{if(!importRaw)return;const link=el('a'),url=URL.createObjectURL(new Blob([importRaw],{type:'application/json'}));link.href=url;link.download='import-'+importSelected+'.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});
byId('import-activate-form').addEventListener('submit',e=>{e.preventDefault();if(!importSelected)return;importAction('/api/imports/'+encodeURIComponent(importSelected)+'/activate',{expected_archive_hash:byId('import-activate-hash').value,expected_events:byId('import-activate-events').value}).catch(failure);});
byId('import-refuse-form').addEventListener('submit',e=>{e.preventDefault();if(!importSelected)return;importAction('/api/imports/'+encodeURIComponent(importSelected)+'/refuse',{expected_archive_hash:byId('import-refuse-hash').value,reason:byId('import-refuse-reason').value}).catch(failure);});
let coverageRequest=0,coverageDetailRequest=0,coverageSelected='',coverageRaw='';
async function loadCoverageTargets(){
 const ticket=++coverageRequest;
 try{const data=await api('/api/coverage').then(r=>r.json());if(ticket!==coverageRequest)return;
  const host=byId('coverage-cards');host.replaceChildren();
  byId('coverage-status').textContent=data.configured?data.targets.length+' configured coverage windows · at most '+data.maximum_targets+'. '+data.limitations:data.limitations;
  for(const t of data.targets){const card=el('article');card.className='card';card.dataset.coverageName=t.name;
   card.append(el('h3',t.name),el('p','Window '+t.start_ms+' … '+t.end_exclusive_ms+' (epoch ms)'),el('p','Gap threshold '+t.gap_threshold_ms+' ms · detail limit '+t.detail_limit));
   const verify=el('button','Verify coverage');verify.addEventListener('click',()=>loadCoverageReport(t.name).catch(failure));card.append(verify);host.append(card);}
 }catch(error){if(ticket===coverageRequest){byId('coverage-cards').replaceChildren();byId('coverage-status').textContent='Coverage configuration unavailable.';failure(error);}}
}
async function loadCoverageReport(name){
 const ticket=++coverageDetailRequest;coverageSelected=name;coverageRaw='';
 byId('coverage-detail').hidden=false;byId('coverage-export').disabled=true;byId('coverage-json').textContent='Verifying coverage report…';byId('coverage-detail-label').textContent=name;
 try{const raw=await api('/api/coverage/'+encodeURIComponent(name)).then(r=>r.text());
  if(ticket!==coverageDetailRequest||name!==coverageSelected)return;
  const data=JSON.parse(raw);coverageRaw=raw;byId('coverage-json').textContent=raw;byId('coverage-export').disabled=false;
  byId('coverage-detail-label').textContent=name+' · '+data.events+' observed events · '+data.months.length+' UTC months · '+data.gap_count+' gaps over threshold · observations only, no approval';
 }catch(error){if(ticket===coverageDetailRequest&&name===coverageSelected){byId('coverage-json').textContent='Verified coverage report unavailable.';byId('coverage-detail-label').textContent=name+' · verification refused';failure(error);}}
}
byId('coverage-refresh').addEventListener('click',loadCoverageTargets);
byId('coverage-detail-refresh').addEventListener('click',()=>{if(coverageSelected)loadCoverageReport(coverageSelected).catch(failure);});
byId('coverage-export').addEventListener('click',()=>{if(!coverageRaw)return;const link=el('a'),url=URL.createObjectURL(new Blob([coverageRaw],{type:'application/json'}));link.href=url;link.download='coverage-'+coverageSelected+'.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});
let auditRequest=0,auditRaw='';
byId('registry-audit-verify').addEventListener('click',async()=>{
 const ticket=++auditRequest;auditRaw='';byId('registry-audit-export').disabled=true;byId('registry-audit-json').textContent='Verifying registry integrity…';byId('registry-audit-status').textContent='Verification in progress';
 try{const raw=await api('/api/registry-audit').then(r=>r.text());if(ticket!==auditRequest)return;
  auditRaw=raw;const data=JSON.parse(raw);byId('registry-audit-json').textContent=raw;
  byId('registry-audit-status').textContent='Integrity '+data.integrity+' · registered '+data.counts.registered+' · completed '+data.counts.completed+' · request-time observation only';
  byId('registry-audit-export').disabled=false;
 }catch(error){if(ticket===auditRequest){byId('registry-audit-json').textContent='Registry integrity verification refused; preserve evidence.';byId('registry-audit-status').textContent='Refused';failure(error);}}
});
byId('registry-audit-export').addEventListener('click',()=>{if(!auditRaw)return;const link=el('a'),url=URL.createObjectURL(new Blob([auditRaw],{type:'application/json'}));link.href=url;link.download='registry-audit.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});
let auditCoverageRequest=0,auditCoverageRaw='';
byId('audit-coverage-verify').addEventListener('click',async()=>{
 const ticket=++auditCoverageRequest;auditCoverageRaw='';byId('audit-coverage-export').disabled=true;byId('audit-coverage-json').textContent='Verifying aggregate audit coverage…';byId('audit-coverage-status').textContent='Verification in progress';
 try{const raw=await api('/api/audit-coverage').then(r=>r.text());if(ticket!==auditCoverageRequest)return;
  auditCoverageRaw=raw;const data=JSON.parse(raw);byId('audit-coverage-json').textContent=raw;
  byId('audit-coverage-status').textContent=data.results_total+' results · '+data.results_with_marker+' with audit marker · '+data.results_without_marker+' without (never coverage) · coverage '+data.coverage.coverage_bps+' bps · diagnostic only';
  byId('audit-coverage-export').disabled=false;
 }catch(error){if(ticket===auditCoverageRequest){byId('audit-coverage-json').textContent='Aggregate audit coverage unavailable.';byId('audit-coverage-status').textContent='Refused';failure(error);}}
});
byId('audit-coverage-export').addEventListener('click',()=>{if(!auditCoverageRaw)return;const link=el('a'),url=URL.createObjectURL(new Blob([auditCoverageRaw],{type:'application/json'}));link.href=url;link.download='audit-coverage.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});
let campaignVerifyRequest=0,campaignVerifyRaw='';
function clearCampaignVerify(){++campaignVerifyRequest;campaignVerifyRaw='';byId('campaign-verify-export').disabled=true;byId('campaign-verify-inputs').disabled=true;byId('campaign-verify-json').hidden=true;byId('campaign-verify-json').textContent='';byId('campaign-verify-status').textContent='';}
byId('campaign-verify-inputs').addEventListener('click',async()=>{
 const id=campaignSelected;if(!id||!campaignRaw)return;
 const ticket=++campaignVerifyRequest;campaignVerifyRaw='';byId('campaign-verify-export').disabled=true;byId('campaign-verify-json').hidden=false;byId('campaign-verify-json').textContent='Verifying frozen campaign inputs…';byId('campaign-verify-status').textContent='Verification in progress';
 try{const raw=await api('/api/campaigns/'+id+'/verify-inputs').then(r=>r.text());
  if(ticket!==campaignVerifyRequest||id!==campaignSelected)return;
  campaignVerifyRaw=raw;byId('campaign-verify-json').textContent=raw;
  byId('campaign-verify-status').textContent=id+' · frozen inputs re-verified against the current plan file; no execution';
  byId('campaign-verify-export').disabled=false;
 }catch(error){if(ticket===campaignVerifyRequest&&id===campaignSelected){byId('campaign-verify-json').textContent='Frozen input verification refused.';byId('campaign-verify-status').textContent='Refused; the frozen plan, build or trial scope changed or is unavailable';failure(error);}}
});
byId('campaign-verify-export').addEventListener('click',()=>{if(!campaignVerifyRaw||!campaignSelected)return;const link=el('a'),url=URL.createObjectURL(new Blob([campaignVerifyRaw],{type:'application/json'}));link.href=url;link.download='campaign-verify-'+campaignSelected+'.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});
function costChart(host,entries){
 host.replaceChildren();
 if(!entries.length){host.append(el('p','No owned cost observations recorded yet.'));return;}
 const rows=entries.map(e=>({label:e.label,cpu:BigInt(e.cpu)}));
 const maxCpu=rows.reduce((m,r)=>r.cpu>m?r.cpu:m,0n);
 if(maxCpu===0n){host.append(el('p','Owned visits recorded; no CPU ticks observed yet. Unobserved visits remain separate.'));return;}
 const height=rows.length*46+30;
 const svg=document.createElementNS('http://www.w3.org/2000/svg','svg');
 svg.setAttribute('viewBox','0 0 1000 '+height);svg.style.height=height+'px';
 svg.setAttribute('role','img');svg.setAttribute('aria-label','Observed owned-visit CPU ticks; exact integers, not equal allocation');
 rows.forEach((r,i)=>{const y=10+i*46;
  const rect=document.createElementNS(svg.namespaceURI,'rect');
  rect.setAttribute('x','10');rect.setAttribute('y',String(y+16));rect.setAttribute('width',String(Math.max(1,Number(r.cpu*900n/maxCpu))));rect.setAttribute('height','18');rect.setAttribute('fill','#9ed7c7');
  const text=document.createElementNS(svg.namespaceURI,'text');
  text.setAttribute('x','10');text.setAttribute('y',String(y+10));text.setAttribute('fill','#dee8ed');text.setAttribute('font-size','12');
  text.textContent=r.label+' · '+r.cpu.toString()+' ticks';
  svg.append(rect,text);});
 host.append(svg,el('p','Observed CPU in raw kernel accounting ticks, not milliseconds. Sampled owned visits only; unobserved visits and other processes are separate. Observations, not enforcement or equal allocation.'));
}
function renderFleetCosts(snapshot){const host=byId('fleet-cost-chart');host.replaceChildren();const obs=snapshot.owned_observations??[];costChart(host,obs.map(o=>({label:'Input '+o.source_index+' · '+o.owned_visits+' owned visits · '+o.cpu_unobserved_visits+' unobserved',cpu:o.observed_cpu_ticks})));for(const o of obs)host.append(el('p','Input '+o.source_index+' · observed parent elapsed '+o.observed_parent_elapsed_ms+' ms · CPU measured on '+o.cpu_observed_visits+' visits'));}
function renderRotationCosts(snapshot){const host=byId('rotation-cost-chart');host.replaceChildren();const members=snapshot.members??[];costChart(host,members.map(m=>({label:m.name+' · '+m.cpu_observed_children+' measured · '+m.cpu_unobserved_children+' unmeasured children',cpu:m.observed_native_cpu_ticks})));for(const m of members)host.append(el('p',m.name+' · observed parent elapsed '+m.observed_parent_elapsed_ms+' ms'));}
}
