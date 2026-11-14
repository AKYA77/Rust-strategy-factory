'use strict';
// Actual service/SQLite/authentication flow; all artifacts are synthetic and disposable.
const {chromium}=require('playwright'),assert=require('node:assert/strict');
const fs=require('node:fs'),os=require('node:os'),path=require('node:path');
const {spawn,spawnSync}=require('node:child_process'),readline=require('node:readline');
const root=path.resolve(__dirname,'..'),binary=process.env.FACTORY_BIN||path.join(root,'target/debug/rust-strategy-factory');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'factory-data-review-'));
const file=name=>path.join(dir,name),password='fixture-dashboard-owner-password',reviewerPassword='fixture-independent-reviewer-password';
let server,browser;
function command(args,input){const r=spawnSync(binary,args,{encoding:'utf8',input});assert.equal(r.status,0,r.stderr);return r.stdout;}
function json(name,value){fs.writeFileSync(file(name),JSON.stringify(value));}
async function start(configured=true){server=spawn(binary,['serve',file('registry.sqlite'),file('owner.secret'),'0',file('catalog.sqlite'),...(configured?[file('reviews.json')]:[])],{stdio:['ignore','pipe','pipe']});let errors='';server.stderr.on('data',d=>errors+=d);return new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(new Error('readiness timeout '+errors)),15000);const lines=readline.createInterface({input:server.stdout});lines.on('line',line=>{let r;try{r=JSON.parse(line);}catch{return;}if(r.listening_port){clearTimeout(timer);resolve('http://127.0.0.1:'+r.listening_port);}});server.once('error',reject);server.once('exit',code=>{clearTimeout(timer);if(code!==0)reject(new Error('server '+code+': '+errors));});});}
async function stop(){if(!server||server.exitCode!==null)return;const p=server;server=undefined;await new Promise((resolve,reject)=>{const timer=setTimeout(()=>{p.kill('SIGKILL');reject(new Error('shutdown timeout'));},10000);p.once('exit',code=>{clearTimeout(timer);code===0?resolve():reject(new Error('shutdown '+code));});p.kill('SIGINT');});}
(async()=>{
 command(['fixture','EURUSD',file('fixture.json')]);const fixture=JSON.parse(fs.readFileSync(file('fixture.json'),'utf8'));json('profile.json',fixture.profile);
 fs.writeFileSync(file('source.csv'),'timestamp_ms,sequence,bid,ask\n0,18446744073709551614,1.00000,1.00010\n1000,18446744073709551615,1.00020,1.00030\n');
 command(['import',file('source.csv'),file('profile.json'),'SYNTHETIC_TEST_ONLY browser review',file('data.json'),'synthetic']);
 const entry=JSON.parse(command(['catalog-record',file('data.json'),file('catalog.sqlite')])).entry,id=entry.receipt.facts.dataset_identity;
 json('calendar.json',{reference:'unapproved synthetic test calendar',scope:{start_ms:0,end_exclusive_ms:1001},open:[{start_ms:0,end_exclusive_ms:1001}]});
 const archive=JSON.parse(command(['archive-evidence',file('source.csv'),file('data.json'),file('catalog.sqlite'),id]));const calendar=JSON.parse(command(['calendar-evidence',file('data.json'),file('catalog.sqlite'),id,file('calendar.json')]));
 json('claim.json',{catalog_hash:entry.hash,archive_report_hash:archive.hash,calendar_report_hash:calendar.hash});
 const admission=JSON.parse(command(['machine-admit',file('source.csv'),file('data.json'),file('catalog.sqlite'),id,file('calendar.json'),file('claim.json')]));
 json('strategy.json',{timeframe:'M1',side:'Long',stop_ticks:100,rule:{AboveMean:{window:100000}}});command(['experiment',file('data.json'),file('strategy.json'),file('registry.sqlite')]);
 command(['auth-init',file('owner.secret')],password);command(['auth-init',file('reviewer.secret')],reviewerPassword);
 json('reviewers.secret',[{name:'independent-fixture-reviewer',password_hash:JSON.parse(fs.readFileSync(file('reviewer.secret'),'utf8')).password_hash,grants:[{symbol:'EURUSD',source:entry.receipt.facts.source,submitter:'fixture-importer',areas:['Source','Profile','Calendar','Quality','Coverage'],expires_ms:Date.now()+86400000}]}]);
 command(['review-authority-init',file('reviewers.secret'),file('authority.secret')]);
 json('reviews.json',{authority:'authority.secret',artifacts:[{machine_id:admission.id,archive:'source.csv',dataset:'data.json'}]});
 let head=null;for(let i=0;i<26;i++){json('decision.json',{machine_id:admission.id,expected_head:head,area:'Source',decision:'Reject',explanation:'synthetic test rejection '+i,evidence_text:'No real quote evidence; fixture only.'});head=JSON.parse(command(['data-review-decide',file('catalog.sqlite'),file('authority.secret'),'independent-fixture-reviewer',file('decision.json')],reviewerPassword)).hash;}
 const original=fs.readFileSync(file('source.csv'));browser=await chromium.launch({headless:true,...(process.env.CHROMIUM_PATH?{executablePath:process.env.CHROMIUM_PATH}:fs.existsSync('/usr/bin/chromium')?{executablePath:'/usr/bin/chromium'}:{})});const context=await browser.newContext();let base=await start(false);
 const page=await context.newPage();page.on('pageerror',error=>console.error('Page error:',error.message));async function login(){await page.goto(base);await page.getByLabel('Password',{exact:true}).fill(password);await Promise.all([page.waitForNavigation(),page.getByRole('button',{name:'Sign in',exact:true}).click()]);await page.locator('#catalog-status').filter({hasText:'Showing'}).waitFor();}
 await login();await page.getByRole('button',{name:'Inspect catalog '+id.slice(0,12),exact:true}).click();await page.locator('#data-review-status').filter({hasText:'not configured'}).waitFor();assert(await page.locator('#review-form').isHidden());
 await stop();base=await start();assert.equal((await context.request.get(base+'/api/data-review/'+id)).status(),401);await login();
 const csrf=(await(await context.request.get(base+'/api/session')).json()).csrf;
 const history=async query=>{const r=await context.request.get(base+'/api/data-review/'+id+(query?'?'+query:''));assert.equal(r.status(),200);return r;};
 const first=await(await history()).json();assert.equal(first.events.length,25);assert.equal(first.head,head);assert(first.current_authority_authenticated);assert.equal(first.admissions[0].id,admission.id);assert(!JSON.stringify(first).includes('password_hash'));
 const last=await(await history('before='+first.next_cursor)).json();assert.equal(last.events.length,1);assert.equal(last.next_cursor,null);
 for(const q of ['limit=0','limit=26','before=0','before=abc','before=01','unknown=1'])assert.equal((await context.request.get(base+'/api/data-review/'+id+'?'+q)).status(),400,q);
 assert.equal((await context.request.get(base+'/api/data-eligibility/'+'f'.repeat(64))).status(),404);
 const payload={reviewer:'independent-fixture-reviewer',password:reviewerPassword,request:{machine_id:admission.id,expected_head:head,area:'Quality',decision:'Revoke',explanation:'Fixture evidence withdrawn',evidence_text:'Synthetic test data cannot qualify.'}};
 assert.equal((await context.request.post(base+'/api/data-review',{data:payload})).status(),403);
 assert.equal((await context.request.post(base+'/api/data-review',{headers:{'X-CSRF-Token':csrf,Origin:'https://example.invalid'},data:payload})).status(),403);
 assert.equal((await context.request.post(base+'/api/data-review',{headers:{'X-CSRF-Token':csrf},data:{...payload,password}})).status(),403);
 assert.equal((await context.request.post(base+'/api/data-review',{headers:{'X-CSRF-Token':csrf},data:{...payload,password:'wrong-independent-password'}})).status(),403);
 for(const viewport of [{width:1280,height:900},{width:390,height:844}]){
  await page.setViewportSize(viewport);await page.getByRole('button',{name:'Inspect catalog '+id.slice(0,12),exact:true}).click();await page.locator('#data-review-status').filter({hasText:'Showing 25 independent'}).waitFor();
  await page.getByRole('button',{name:'Older review decisions',exact:true}).click();await page.locator('#data-review-status').filter({hasText:'Showing 1 independent'}).waitFor();assert(await page.locator('#review-submit').isDisabled());
  await page.getByRole('button',{name:'Latest review history',exact:true}).click();await page.locator('#data-review-status').filter({hasText:'Showing 25 independent'}).waitFor();
  const raw=await(await history()).text(),download=page.waitForEvent('download');await page.getByRole('button',{name:'Export review history page',exact:true}).click();assert.equal(fs.readFileSync(await(await download).path(),'utf8'),raw);
  await page.getByRole('button',{name:'Revalidate artifacts and data gates',exact:true}).click();await page.locator('#review-live-status').filter({hasText:'DATA_BLOCKED'}).waitFor();assert((await page.locator('#review-live-status').textContent()).includes('Exact current artifacts revalidated'));assert((await page.locator('#review-live-status').textContent()).includes('Synthetic'));
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1));
  fs.mkdirSync(path.join(root,'state/browser'),{recursive:true});await page.screenshot({path:path.join(root,'state/browser/data-review-'+viewport.width+'.png'),fullPage:true});
 }
 await page.getByLabel('Review area',{exact:true}).selectOption('Quality');await page.getByLabel('Decision',{exact:true}).selectOption('Revoke');await page.getByLabel('Decision explanation').fill('Fixture evidence withdrawn');await page.getByLabel('Retained review evidence').fill('Synthetic test data cannot qualify.');await page.getByLabel('Independent reviewer identity').fill('independent-fixture-reviewer');await page.getByLabel('Independent reviewer password').fill(reviewerPassword);
 await page.getByRole('button',{name:'Record independent decision',exact:true}).click();await page.locator('#data-review-status').filter({hasText:'Independent decision recorded'}).waitFor();assert.equal(await page.getByLabel('Independent reviewer password').inputValue(),'');const recorded=await(await history()).json();assert.equal(recorded.events[0].record.request.decision,'Revoke');assert.equal(recorded.events[0].record.reviewer,'independent-fixture-reviewer');
 assert.equal((await context.request.post(base+'/api/data-review',{headers:{'X-CSRF-Token':csrf},data:payload})).status(),409);
 const current=recorded.head;assert.equal((await context.request.post(base+'/api/data-review',{headers:{'X-CSRF-Token':csrf},data:{...payload,request:{...payload.request,expected_head:current,decision:'Approve'}}})).status(),409);
 assert.equal((await(await history()).json()).head,current);
 fs.appendFileSync(file('source.csv'),'\n');await page.getByRole('button',{name:'Revalidate artifacts and data gates',exact:true}).click();await page.locator('#review-live-status').filter({hasText:'verification failed'}).waitFor();assert.equal((await(await context.request.get(base+'/api/data-eligibility/'+admission.id)).json()).live_machine_revalidated,false);
 fs.writeFileSync(file('source.csv'),original);await stop();base=await start();await login();assert.equal((await(await history()).json()).head,current);
 // The configured root is reloaded for every request, so removal/corruption cannot retain approval.
 fs.chmodSync(file('authority.secret'),0o644);assert.equal((await context.request.get(base+'/api/data-review/'+id)).status(),500);await page.getByRole('button',{name:'Inspect catalog '+id.slice(0,12),exact:true}).click();await page.locator('#data-review-status').filter({hasText:'unavailable'}).waitFor();assert(await page.locator('#review-form').isHidden());
 assert.equal((await context.request.get(base+'/api/data-eligibility/'+admission.id)).status(),500);
 console.log(JSON.stringify({browser:'Chromium',synthetic:true,passed:['legacy unconfigured reviews','owner authentication and CSRF/origin','independent credential and scope enforcement','immutable decision pages and exact export','synthetic approval refusal','actual reviewer revocation and stale-head refusal','live changed-artifact refusal','desktop/mobile layout','restart history','current authority permission failure']}));
})().catch(async error=>{console.error(error);const page=browser?.contexts()[0]?.pages()[0];if(page){console.error(await page.locator('#error').textContent());console.error(await page.locator('#data-review-status').textContent());}process.exitCode=1;}).finally(async()=>{if(browser)await browser.close();try{await stop();}catch(error){console.error(error);process.exitCode=1;}fs.rmSync(dir,{recursive:true,force:true});});
