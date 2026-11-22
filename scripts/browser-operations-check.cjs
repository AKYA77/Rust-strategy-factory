'use strict';
// Development-only integration against the actual Rust service and real SQLite fixture state.
const {chromium}=require('playwright');
const assert=require('node:assert/strict');
const fs=require('node:fs');const os=require('node:os');const path=require('node:path');
const {spawn,spawnSync}=require('node:child_process');const readline=require('node:readline');
const root=path.resolve(__dirname,'..');const binary=process.env.FACTORY_BIN||path.join(root,'target/debug/rust-strategy-factory');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'factory-operations-browser-'));const db=path.join(dir,'state.sqlite'),auth=path.join(dir,'auth.json');
const catalog=path.join(dir,'catalog.sqlite'),operations=path.join(dir,'operations.json');
const password='fixture-only-password-not-for-production';let server,browser;
function command(args,input){const r=spawnSync(binary,args,{encoding:'utf8',input});assert.equal(r.status,0,r.stderr||String(r.error));return r.stdout;}
async function start(){server=spawn(binary,['serve',db,auth,'0',catalog,'--operations',operations],{stdio:['ignore','pipe','pipe']});let errors='';server.stderr.on('data',d=>errors+=d);return new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(new Error('server readiness timed out: '+errors)),15000);const lines=readline.createInterface({input:server.stdout});lines.on('line',line=>{let r;try{r=JSON.parse(line);}catch{return;}if(r.listening_port){clearTimeout(timer);resolve('http://127.0.0.1:'+r.listening_port);}});server.once('error',e=>{clearTimeout(timer);reject(e);});server.once('exit',code=>{clearTimeout(timer);if(code!==0)reject(new Error('server failed '+code+': '+errors));});});}
async function stop(){if(!server||server.exitCode!==null)return;const child=server;server=undefined;await new Promise((resolve,reject)=>{const timer=setTimeout(()=>{child.kill('SIGKILL');reject(new Error('graceful shutdown timed out'));},10000);child.once('exit',code=>{clearTimeout(timer);code===0?resolve():reject(new Error('server shutdown '+code));});child.kill('SIGINT');});}
(async()=>{
 const profile=JSON.parse(command(['profiles'])).find(p=>p.symbol==='EURUSD');fs.writeFileSync(path.join(dir,'profile.json'),JSON.stringify(profile));
 const epoch=1735689600000;const rows=[];for(let i=0;i<8;i++)rows.push([epoch+i*60000,i,'1.00000','1.00010'].join(','));
 const csv=path.join(dir,'source.csv');fs.writeFileSync(csv,'timestamp_ms,sequence,bid,ask\n'+rows.join('\n')+'\n');
 const completeDir=path.join(dir,'import-complete'),partialDir=path.join(dir,'import-partial'),emptyDir=path.join(dir,'import-empty');
 fs.mkdirSync(completeDir);fs.mkdirSync(partialDir);fs.mkdirSync(emptyDir);
 const sourceLabel='SYNTHETIC_TEST_ONLY operations browser';
 command(['import-step',csv,path.join(dir,'profile.json'),sourceLabel,completeDir,'synthetic','4']);
 command(['import-step',csv,path.join(dir,'profile.json'),sourceLabel,completeDir,'synthetic','4']);
 command(['import-step',csv,path.join(dir,'profile.json'),sourceLabel,partialDir,'synthetic','4']);
 const completeStatus=JSON.parse(command(['import-status',completeDir]));assert.equal(completeStatus.complete,true);assert.equal(completeStatus.events,8);
 const partialStatus=JSON.parse(command(['import-status',partialDir]));assert.equal(partialStatus.complete,false);assert.equal(partialStatus.events,4);
 command(['fixture','EURUSD',path.join(dir,'input.json')]);command(['pack-native',path.join(dir,'input.json'),path.join(dir,'coverage.rsf')]);
 fs.writeFileSync(path.join(dir,'strategy.json'),JSON.stringify({timeframe:'M1',side:'Long',stop_ticks:50,rule:{AboveMean:{window:2}}}));
 const experiment=JSON.parse(command(['experiment',path.join(dir,'input.json'),path.join(dir,'strategy.json'),db]));
 command(['catalog-record',path.join(dir,'input.json'),catalog]);
 fs.writeFileSync(operations,JSON.stringify({imports:[{name:'import-complete',directory:'import-complete'},{name:'import-partial',directory:'import-partial'},{name:'import-empty',directory:'import-empty'}],coverage:[{name:'month-window',dataset:'coverage.rsf',start_ms:epoch,end_exclusive_ms:1738368000000,gap_threshold_ms:60000},{name:'full-span',dataset:'coverage.rsf',start_ms:epoch,end_exclusive_ms:1769904000000,gap_threshold_ms:60000}]}));
 command(['auth-init',auth],password+'\n');
 // Invalid operations configuration refuses startup before any listener exists.
 const badOperations=path.join(dir,'bad-operations.json');fs.writeFileSync(badOperations,JSON.stringify({imports:[{name:'Bad_Name',directory:'import-complete'}]}));
 const badStart=spawnSync(binary,['serve',db,auth,'0',catalog,'--operations',badOperations],{encoding:'utf8',timeout:10000});assert.notEqual(badStart.status,0);
 let base=await start();browser=await chromium.launch({headless:true,...(process.env.CHROMIUM_PATH?{executablePath:process.env.CHROMIUM_PATH}:fs.existsSync('/usr/bin/chromium')?{executablePath:'/usr/bin/chromium'}:{})});
 const context=await browser.newContext({viewport:{width:1280,height:900},acceptDownloads:true});const page=await context.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
 const importCard=name=>page.locator('#import-cards [data-import-name="'+name+'"]');
 const coverageCard=name=>page.locator('#coverage-cards [data-coverage-name="'+name+'"]');
 for(const endpoint of ['/api/imports','/api/imports/import-complete','/api/coverage','/api/coverage/month-window','/api/registry-audit','/api/audit-coverage'])assert.equal((await context.request.get(base+endpoint)).status(),401,endpoint);
 assert.equal((await context.request.post(base+'/api/imports/import-complete/activate',{data:{expected_archive_hash:'0'.repeat(64),expected_events:'8'}})).status(),401);
 assert.equal((await context.request.post(base+'/api/imports/import-complete/refuse',{data:{expected_archive_hash:'0'.repeat(64),reason:'unauthorized'}})).status(),401);
 async function login(){await page.goto(base);await page.getByLabel('Password').fill(password);await Promise.all([page.waitForNavigation(),page.getByRole('button',{name:'Sign in',exact:true}).click()]);await page.locator('#imports-status').filter({hasText:'configured import directories'}).waitFor();}
 await login();const session=await(await context.request.get(base+'/api/session')).json();const headers={'X-CSRF-Token':session.csrf};
 assert.equal((await context.request.post(base+'/api/imports/import-complete/activate',{data:{expected_archive_hash:completeStatus.configuration.archive_hash,expected_events:'8'}})).status(),403);
 assert.equal((await context.request.post(base+'/api/imports/import-complete/activate',{headers:{...headers,Origin:'https://untrusted.example'},data:{expected_archive_hash:completeStatus.configuration.archive_hash,expected_events:'8'}})).status(),403);
 assert.equal((await context.request.post(base+'/api/imports/import-complete/activate',{headers,data:{expected_archive_hash:completeStatus.configuration.archive_hash,expected_events:'8',approved:true}})).status(),422);
 assert.equal((await context.request.post(base+'/api/imports/import-complete/refuse',{headers,data:{expected_archive_hash:completeStatus.configuration.archive_hash,reason:'x'.repeat(9000)}})).status(),413);
 assert.equal((await context.request.post(base+'/api/imports',{headers,data:{}})).status(),405);
 assert.equal((await context.request.post(base+'/api/registry-audit',{headers,data:{}})).status(),405);
assert.equal((await context.request.post(base+'/api/audit-coverage',{headers,data:{}})).status(),405);
 assert.equal((await context.request.post(base+'/api/coverage/month-window',{headers,data:{}})).status(),405);
 assert.equal((await context.request.get(base+'/api/imports/not-configured')).status(),404);
 assert.equal((await context.request.get(base+'/api/coverage/not-configured')).status(),404);
 assert.equal((await context.request.get(base+'/api/campaigns/not-a-hash/verify-inputs')).status(),400);
 // Verified inventory: exact states, no local path leakage, byte-exact export.
 const inventoryRaw=await(await context.request.get(base+'/api/imports')).text();assert(!inventoryRaw.includes(dir));
 const inventory=JSON.parse(inventoryRaw);assert.equal(inventory.configured,true);assert.equal(inventory.imports.length,3);
 assert.equal(inventory.imports[0].state,'complete');assert.equal(inventory.imports[0].events,'8');assert.equal(inventory.imports[0].chunks,'2');
 assert.equal(inventory.imports[1].state,'in_progress');assert.equal(inventory.imports[1].events,'4');
 assert.equal(inventory.imports[2].state,'not_initialized');
 await importCard('import-complete').filter({hasText:'State: complete'}).waitFor();
 await importCard('import-partial').filter({hasText:'State: in_progress'}).waitFor();
 await importCard('import-empty').filter({hasText:'State: not_initialized'}).waitFor();
 assert(await importCard('import-empty').getByRole('button',{name:'Inspect import receipt',exact:true}).isDisabled());
 const inventoryDownload=page.waitForEvent('download');await page.getByRole('button',{name:'Export import inventory',exact:true}).click();assert.equal(fs.readFileSync(await(await inventoryDownload).path(),'utf8'),inventoryRaw);
 // Inspect the complete import: exact receipt export and prefilled confirmation.
 await importCard('import-complete').getByRole('button',{name:'Inspect import receipt',exact:true}).click();
 await page.locator('#import-json').filter({hasText:completeStatus.configuration.archive_hash}).waitFor();
 const detailRaw=await page.locator('#import-json').textContent();assert.equal(JSON.parse(detailRaw).progress.events,8);assert(!detailRaw.includes(dir));
 const detailDownload=page.waitForEvent('download');await page.getByRole('button',{name:'Export import receipt',exact:true}).click();assert.equal(fs.readFileSync(await(await detailDownload).path(),'utf8'),detailRaw);
 assert.equal(await page.locator('#import-activate-hash').inputValue(),completeStatus.configuration.archive_hash);
 assert.equal(await page.locator('#import-activate-events').inputValue(),'8');
 assert(await page.locator('#import-activate-form').isVisible());assert(await page.locator('#import-refuse-form').isVisible());
 // Stale confirmation refuses; the exact verified identity activates a catalog observation only.
 assert.equal((await context.request.post(base+'/api/imports/import-complete/activate',{headers,data:{expected_archive_hash:'0'.repeat(64),expected_events:'8'}})).status(),409);
 assert.equal((await context.request.post(base+'/api/imports/import-complete/activate',{headers,data:{expected_archive_hash:completeStatus.configuration.archive_hash,expected_events:'9'}})).status(),409);
 await page.locator('#import-activate-hash').fill('0'.repeat(64));
 await page.getByRole('button',{name:'Confirm activation into catalog',exact:true}).click();
 await page.locator('#import-decision-status').filter({hasText:'Decision refused'}).waitFor();
 await page.getByRole('button',{name:'Refresh selected import',exact:true}).click();
 await page.locator('#import-json').filter({hasText:completeStatus.configuration.archive_hash}).waitFor();
 await page.getByRole('button',{name:'Confirm activation into catalog',exact:true}).click();
 await page.locator('#import-decision-status').filter({hasText:'Decision recorded: activated'}).waitFor();
 const decisionsAfter=JSON.parse(await page.locator('#import-json').textContent()).decisions;assert.equal(decisionsAfter.length,1);assert.equal(decisionsAfter[0].kind,'activated');assert.equal(decisionsAfter[0].declared_class,'SyntheticTestOnly');
 const identity=decisionsAfter[0].dataset_identity;assert.match(identity,/^[a-f0-9]{64}$/);
 const entry=await(await context.request.get(base+'/api/catalog/'+identity)).json();assert.equal(entry.receipt.facts.events,8);assert.equal(entry.receipt.facts.declared_class,'SYNTHETIC_TEST_ONLY');assert(entry.receipt.limitations.includes('not catalog approval'));
 const repeat=await(await context.request.post(base+'/api/imports/import-complete/activate',{headers,data:{expected_archive_hash:completeStatus.configuration.archive_hash,expected_events:'8'}})).json();assert.equal(repeat.reused,true);
 // Explicit operator refusal retains evidence and changes no artifact.
 const partialChunk=path.join(partialDir,'chunk-000000.rsf');const partialBytes=fs.readFileSync(partialChunk);
 await importCard('import-partial').getByRole('button',{name:'Inspect import receipt',exact:true}).click();
 await page.locator('#import-json').filter({hasText:partialStatus.configuration.archive_hash}).waitFor();
 assert(await page.locator('#import-activate-form').isHidden());
 await page.locator('#import-refuse-reason').fill('source revision superseded; retained for audit');
 await page.getByRole('button',{name:'Record refusal',exact:true}).click();
 await page.locator('#import-decision-status').filter({hasText:'Decision recorded: refused'}).waitFor();
 assert.deepEqual(fs.readFileSync(partialChunk),partialBytes);
 await page.getByRole('button',{name:'Verify import inventory',exact:true}).click();
 await importCard('import-partial').filter({hasText:'Last decision: refused'}).waitFor();
 // Tampered chunks refuse inventory, detail, export and activation; refusal stays reachable.
 const completeChunk=path.join(completeDir,'chunk-000000.rsf');const tampered=fs.readFileSync(completeChunk);tampered[tampered.length-1]^=1;fs.writeFileSync(completeChunk,tampered);
 await page.getByRole('button',{name:'Verify import inventory',exact:true}).click();
 await importCard('import-complete').filter({hasText:'State: refused'}).waitFor();
 assert((await importCard('import-complete').textContent()).includes('mismatch'));
 await importCard('import-complete').getByRole('button',{name:'Inspect import receipt',exact:true}).click();
 await page.locator('#import-detail-label').filter({hasText:'verification refused'}).waitFor();
 assert(await page.getByRole('button',{name:'Export import receipt',exact:true}).isDisabled());
 assert(await page.locator('#import-refuse-form').isVisible());
 assert.equal((await context.request.post(base+'/api/imports/import-complete/activate',{headers,data:{expected_archive_hash:completeStatus.configuration.archive_hash,expected_events:'8'}})).status(),409);
 await page.locator('#import-refuse-hash').fill('0'.repeat(64));
 await page.locator('#import-refuse-reason').fill('tampered chunk retained for investigation');
 await page.getByRole('button',{name:'Record refusal',exact:true}).click();
 await page.locator('#import-decision-status').filter({hasText:'Decision recorded: refused'}).waitFor();
 const decisionFile=fs.readFileSync(path.join(completeDir,'activation-decisions.jsonl'),'utf8').trim().split('\n').map(l=>JSON.parse(l));
 assert.equal(decisionFile.length,3);assert.equal(decisionFile[2].verification,'unverified');assert(decisionFile[2].verification_error.includes('mismatch'));assert(!JSON.stringify(decisionFile).includes(dir));
 // Corrupt decision history refuses further actions without deleting evidence.
 fs.writeFileSync(path.join(partialDir,'activation-decisions.jsonl'),'{"forged":true}\n');
 await page.getByRole('button',{name:'Verify import inventory',exact:true}).click();
 await importCard('import-partial').filter({hasText:'Decision history corrupt'}).waitFor();
 assert.equal((await context.request.post(base+'/api/imports/import-partial/refuse',{headers,data:{expected_archive_hash:partialStatus.configuration.archive_hash,reason:'after corruption'}})).status(),409);
 // Coverage observations: exact months/events/gaps, bounded details, byte-exact export.
 await coverageCard('month-window').getByRole('button',{name:'Verify coverage',exact:true}).click();
 await page.locator('#coverage-detail-label').filter({hasText:'1000 observed events · 1 UTC months · 0 gaps'}).waitFor();
 const monthRaw=await page.locator('#coverage-json').textContent();const monthReport=JSON.parse(monthRaw);
 assert.equal(monthReport.events,1000);assert.equal(monthReport.gap_count,0);assert(monthReport.limitations.includes('No acquisition'));assert(!monthRaw.includes(dir));
 const monthDownload=page.waitForEvent('download');await page.getByRole('button',{name:'Export coverage report',exact:true}).click();assert.equal(fs.readFileSync(await(await monthDownload).path(),'utf8'),monthRaw);
 await coverageCard('full-span').getByRole('button',{name:'Verify coverage',exact:true}).click();
 await page.locator('#coverage-detail-label').filter({hasText:'2000 observed events · 13 UTC months · 1 gaps'}).waitFor();
 const spanRaw=await page.locator('#coverage-json').textContent();const spanReport=JSON.parse(spanRaw);
 assert.equal(spanReport.gaps[0].elapsed_ms,31526010000);assert.equal(spanReport.gap_details_truncated,false);
 const spanDownload=page.waitForEvent('download');await page.getByRole('button',{name:'Export coverage report',exact:true}).click();assert.equal(fs.readFileSync(await(await spanDownload).path(),'utf8'),spanRaw);
 // A delayed older coverage response cannot overwrite the newer selection.
 let coverageArrived,coverageRelease,coverageFinished;const coverageSeen=new Promise(r=>coverageArrived=r),coverageGate=new Promise(r=>coverageRelease=r),coverageDone=new Promise(r=>coverageFinished=r);
 await page.route('**/api/coverage/full-span',async route=>{const response=await route.fetch();coverageArrived();await coverageGate;await route.fulfill({response});coverageFinished();});
 await coverageCard('full-span').getByRole('button',{name:'Verify coverage',exact:true}).click();await coverageSeen;
 await coverageCard('month-window').getByRole('button',{name:'Verify coverage',exact:true}).click();
 await page.locator('#coverage-detail-label').filter({hasText:'1000 observed events'}).waitFor();
 coverageRelease();await coverageDone;await page.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));
 assert.equal(JSON.parse(await page.locator('#coverage-json').textContent()).name,'month-window');
 assert(await page.getByRole('button',{name:'Export coverage report',exact:true}).isEnabled());
 await page.unroute('**/api/coverage/full-span');
 // Registry integrity audit and extended exact resource observations.
 const overview=await(await context.request.get(base+'/api/overview')).json();
 await page.getByRole('button',{name:'Verify registry integrity',exact:true}).click();
 await page.locator('#registry-audit-status').filter({hasText:'Integrity ok'}).waitFor();
 const auditRaw=await page.locator('#registry-audit-json').textContent();const audit=JSON.parse(auditRaw);
 assert.equal(audit.integrity,'ok');assert.equal(audit.counts.registered,overview.counts.registered);assert(audit.scope.includes('Not a continuous audit'));
 const auditDownload=page.waitForEvent('download');await page.getByRole('button',{name:'Export registry audit',exact:true}).click();assert.equal(fs.readFileSync(await(await auditDownload).path(),'utf8'),auditRaw);
 // Aggregate audited-result coverage: marker absence is reported explicitly, never coverage.
 await page.getByRole('button',{name:'Verify aggregate coverage',exact:true}).click();
 await page.locator('#audit-coverage-status').filter({hasText:'with audit marker'}).waitFor();
 const coverageRaw=await page.locator('#audit-coverage-json').textContent();const coverage=JSON.parse(coverageRaw);
 assert.equal(coverage.version,'registry-audit-coverage-v1');
 assert.equal(coverage.results_with_marker+coverage.results_without_marker,coverage.results_total);
 assert(coverage.results_total>=1);assert(coverage.results_with_marker>=1);
 assert.equal(coverage.coverage.candidates,coverage.results_with_marker);
 assert(coverage.limitations.includes('never treated as coverage'));
 const coverageDownload=page.waitForEvent('download');await page.getByRole('button',{name:'Export aggregate coverage',exact:true}).click();assert.equal(fs.readFileSync(await(await coverageDownload).path(),'utf8'),coverageRaw);
 const resources=await(await context.request.get(base+'/api/resources')).json();
 for(const field of ['peak_rss_bytes','process_user_ticks','process_system_ticks','process_start_ticks'])assert.equal(typeof resources[field],'string',field);
 assert(BigInt(resources.peak_rss_bytes)>0n);assert(resources.cpu_scope.includes('not milliseconds'));
 assert.equal(await page.locator('#resources article').count(),6);
 assert((await page.locator('#resource-status').textContent()).includes('not milliseconds'));
 // Mobile layout: no horizontal overflow with the new panels populated.
 fs.mkdirSync(path.join(root,'state/browser'),{recursive:true});
 await page.setViewportSize({width:390,height:844});
 assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1));
 await page.screenshot({path:path.join(root,'state/browser/operations-mobile.png'),fullPage:true});
 await page.setViewportSize({width:1280,height:900});
 await page.screenshot({path:path.join(root,'state/browser/operations-desktop.png'),fullPage:true});
 // Restart: verified states, retained decisions and catalog observations persist.
 await stop();base=await start();await login();
 await importCard('import-complete').filter({hasText:'State: refused'}).waitFor();
 await importCard('import-complete').filter({hasText:'Last decision: refused'}).waitFor();
 await importCard('import-partial').filter({hasText:'Decision history corrupt'}).waitFor();
 assert.equal((await(await context.request.get(base+'/api/catalog/'+identity)).json()).receipt.facts.events,8);
 await coverageCard('month-window').getByRole('button',{name:'Verify coverage',exact:true}).click();
 await page.locator('#coverage-detail-label').filter({hasText:'1000 observed events'}).waitFor();
 // Corrupt registry refuses the integrity audit without fabricating a pass.
 const corrupt=spawnSync('python3',['-c',"import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute('UPDATE experiments SET key_json=? WHERE id=?',('{}',sys.argv[2])); c.commit()",db,experiment.experiment_id],{encoding:'utf8'});assert.equal(corrupt.status,0,corrupt.stderr);
 assert.equal((await context.request.get(base+'/api/registry-audit')).status(),500);
 await page.getByRole('button',{name:'Verify registry integrity',exact:true}).click();
 await page.locator('#registry-audit-status').filter({hasText:'Refused'}).waitFor();
 assert((await page.locator('#registry-audit-json').textContent()).includes('preserve evidence'));
 assert(await page.getByRole('button',{name:'Export registry audit',exact:true}).isDisabled());
 // The aggregate coverage read refuses the same corrupt registry instead of fabricating a pass.
 assert.equal((await context.request.get(base+'/api/audit-coverage')).status(),500);
 await page.getByRole('button',{name:'Verify aggregate coverage',exact:true}).click();
 await page.locator('#audit-coverage-status').filter({hasText:'Refused'}).waitFor();
 assert(await page.getByRole('button',{name:'Export aggregate coverage',exact:true}).isDisabled());
 assert.deepEqual(errors,[]);
 console.log(JSON.stringify({browser:'Chromium',synthetic:true,passed:['authenticated bounded import inventory with exact verified states','auth/CSRF/origin/body/unknown-field/read-only enforcement','byte-exact inventory/receipt/coverage/audit exports without local paths','stale-confirmation activation refusal and explicit catalog-observation activation','declared class never approval; idempotent reuse','explicit operator refusal retains evidence and preserves artifacts','tampered chunk refuses inventory/detail/activation; unverified refusal retained','corrupt decision history refuses actions','exact bounded coverage months/events/gaps and stale-response ownership','request-time registry integrity audit and corrupt-registry refusal','aggregate audited-result coverage with explicit marker-absence accounting and corrupt-registry refusal','exact peak-RSS/CPU-tick resource observations','desktop/mobile/restart persistence']}));
})().catch(e=>{console.error(e);process.exitCode=1;}).finally(async()=>{if(browser)await browser.close();try{await stop();}catch(e){console.error(e);process.exitCode=1;}fs.rmSync(dir,{recursive:true,force:true});});
