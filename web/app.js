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
const symbols=['EURUSD','GBPUSD','USDJPY','USDCHF','USDCAD','AUDUSD','NZDUSD','XAUUSD'];
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

function renderProtocols(records){byId('protocols').replaceChildren();if(!records.length)byId('protocols').append(el('p','No frozen protocols registered.'));for(const p of records){const button=el('button',p.id.slice(0,12)+(p.report_ready?' · inspect evidence':' · incomplete'));button.disabled=!p.report_ready;button.addEventListener('click',()=>api('/api/protocols/'+p.id).then(r=>r.text()).then(text=>{byId('protocol-detail').hidden=false;byId('protocol-detail').textContent=text;}).catch(failure));byId('protocols').append(button);}}
function renderRuns(records){byId('runs').replaceChildren();if(!records.length)byId('runs').append(el('p','No registered research runs.'));for(const run of records){const card=el('article');card.className='card';card.append(el('h3',run.id.slice(0,12)),el('p',run.state+' · '+(run.active?'task lease active':'no active task lease')+' · cursor '+run.cursor),el('p','Base tick simulations '+run.executed+' · reused '+run.reused+' · rejected '+run.rejected+' · failed '+run.failed));if(run.improvements)card.append(el('p','Improvement jobs queued '+run.improvements.queued+' · completed '+run.improvements.completed+' · failed '+run.improvements.failed+' · child receipt links '+run.improvements.child_receipt_links));if(run.last_error)card.append(el('p',run.last_error));const controls=el('div');controls.className='controls';for(const [label,state]of [['Pause','paused'],['Resume','running'],['Stop','stopped']]){const button=el('button',label);button.addEventListener('click',()=>api('/api/runs/'+run.id+'/control',{state}).then(refresh).catch(failure));controls.append(button);}card.append(controls);byId('runs').append(card);}}
const protocolPager=ledgerPager("protocol",renderProtocols),runPager=ledgerPager("run",renderRuns);
let generatedSearchRequest=0,generatedDetailRequest=0,generatedSelected='',generatedRaw='';
async function loadGeneratedSearches(){
 const request=++generatedSearchRequest;try{const rows=await api('/api/corpus-searches').then(r=>r.json());if(request!==generatedSearchRequest)return;const host=byId('generated-search-cards');host.replaceChildren();byId('generated-search-status').textContent=rows.length+' registered generated searches. Slot counts are not new tick simulations.';
 for(const search of rows){const card=el('article');card.className='card';card.dataset.searchId=search.search_id;card.append(el('h3',search.search_id.slice(0,12)),el('p',search.state+' · generated slots '+search.generated_slots+' of '+search.maximum_candidates),el('p',search.lease_active?'Coordinator lease active':'No active coordinator lease'),el('p','Completed slots '+(search.slot_states.completed??0)+' · pending '+((search.slot_states.queued??0)+(search.slot_states.pending??0))+' · errors '+(search.slot_states.error??0)));const controls=el('div');controls.className='controls';
 for(const [label,state]of [['Pause generated search','paused'],['Resume generated search','running'],['Stop generated search permanently','stopped']]){const button=el('button',label);button.disabled=['stopped','exhausted'].includes(search.state)||(state==='paused'&&search.state!=='running')||(state==='running'&&search.state!=='paused');button.addEventListener('click',async()=>{button.disabled=true;try{await api('/api/corpus-searches/'+search.search_id+'/control',{state});await loadGeneratedSearches();if(generatedSelected===search.search_id)await loadGeneratedSearchDetail(search.search_id);}catch(error){await loadGeneratedSearches();failure(error);}});controls.append(button);}
 const inspect=el('button','Inspect generated search');inspect.addEventListener('click',()=>loadGeneratedSearchDetail(search.search_id));controls.append(inspect);card.append(controls);host.append(card);}
 }catch(error){if(request===generatedSearchRequest){byId('generated-search-cards').replaceChildren();byId('generated-search-status').textContent='Generated search inventory unavailable; no verified state established.';failure(error);}}
}
async function loadGeneratedSearchDetail(id){const request=++generatedDetailRequest;generatedSelected=id;generatedRaw='';byId('generated-search-export').disabled=true;byId('generated-search-detail').hidden=false;byId('generated-search-json').textContent='Verifying generated search snapshot…';
 try{const raw=await api('/api/corpus-searches/'+id).then(r=>r.text());if(request!==generatedDetailRequest)return;generatedRaw=raw;byId('generated-search-json').textContent=raw;byId('generated-search-export').disabled=false;}catch(error){if(request===generatedDetailRequest){byId('generated-search-json').textContent='Verified generated search snapshot unavailable.';failure(error);}}
}
byId('generated-search-refresh').addEventListener('click',loadGeneratedSearches);
byId('generated-search-detail-refresh').addEventListener('click',()=>{if(generatedSelected)loadGeneratedSearchDetail(generatedSelected);});
byId('generated-search-export').addEventListener('click',()=>{if(!generatedRaw)return;const link=el('a'),url=URL.createObjectURL(new Blob([generatedRaw],{type:'application/json'}));link.href=url;link.download='corpus-search-'+generatedSelected+'.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});
async function loadResources(){
 try {const data=await api('/api/resources').then(r=>r.json());const panel=byId('resources');panel.replaceChildren();for(const [label,value]of [['Dashboard service RSS',data.process_rss_bytes+' bytes'],['Host available memory',data.host_available_memory_bytes+' bytes'],['Available parallelism',data.available_parallelism]]){const card=el('article');card.className='card';card.append(el('h3',label),el('p',value));panel.append(card);}byId('resource-status').textContent='Current dashboard process and host observations. Standalone research processes have separate memory use; these observations do not enforce OS limits.';}
 catch(_error){byId('resources').replaceChildren();byId('resource-status').textContent='Runtime resource observations unavailable.';}
}
async function refresh(){const [overview]=await Promise.all([api("/api/overview?runs=false").then(r=>r.json()),protocolPager.load(),runPager.load(),loadHistory(),loadCatalog(),loadResources(),loadGeneratedSearches()]);renderAuditHealth(overview.audit_health);currentEngine=overview.engine;byId('readiness').textContent=overview.software_status+' · '+overview.historical_readiness;byId('qualification').textContent=overview.qualification;byId('metrics').replaceChildren();for(const [label,value]of Object.entries(overview.counts)){const card=el('div');card.className='card';card.append(el('span',({registered:'Registered experiments',completed:'Committed tick results',running:'Running experiment records',attempts:'Lease acquisitions'})[label]||label),Object.assign(el('strong',value),{className:'metric'}));byId('metrics').append(card);}renderResults();}
function renderDatasets(){const seen=new Set();byId('datasets').replaceChildren();for(const row of rows){const m=row.manifest;if(!m)continue;const key=JSON.stringify([m.content_hash,m.profile_hash,m.archive_hash,m.class,m.source,m.parser_version]);if(seen.has(key))continue;seen.add(key);const card=el('article');card.className='card';card.append(el('h3',m.symbol),el('p',m.class),el('p',m.events+' quote events'),el('p',new Date(m.first_ms).toISOString()+' → '+new Date(m.last_ms).toISOString()),el('p',m.source),el('p','Content '+m.content_hash));byId('datasets').append(card);}if(!seen.size)byId('datasets').append(el('p','No provenance manifests in this experiment page.'));}
(async()=>{csrf=(await api('/api/session').then(r=>r.json())).csrf;await refresh();setInterval(()=>refresh().catch(failure),10000);})().catch(failure);

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

}
