'use strict';
// Development-only integration against the actual Rust service and real SQLite fixture state.
const {chromium}=require('playwright');
const assert=require('node:assert/strict');
const fs=require('node:fs');const os=require('node:os');const path=require('node:path');
const {spawn,spawnSync}=require('node:child_process');const readline=require('node:readline');
const root=path.resolve(__dirname,'..');const binary=process.env.FACTORY_BIN||path.join(root,'target/debug/rust-strategy-factory');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'factory-history-browser-'));const db=path.join(dir,'state.sqlite'),auth=path.join(dir,'auth.json');
const catalog=path.join(dir,'catalog.sqlite');let catalogConfigured=false;
const password='fixture-only-password-not-for-production';let server,browser;
function command(args,input){const r=spawnSync(binary,args,{encoding:'utf8',input});assert.equal(r.status,0,r.stderr);return r.stdout;}
async function start(){server=spawn(binary,['serve',db,auth,'0',...(catalogConfigured?[catalog]:[])],{stdio:['ignore','pipe','pipe']});let errors='';server.stderr.on('data',d=>errors+=d);return new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(new Error('server readiness timed out: '+errors)),15000);const lines=readline.createInterface({input:server.stdout});lines.on('line',line=>{let r;try{r=JSON.parse(line);}catch{return;}if(r.listening_port){clearTimeout(timer);resolve('http://127.0.0.1:'+r.listening_port);}});server.once('error',e=>{clearTimeout(timer);reject(e);});server.once('exit',code=>{clearTimeout(timer);if(code!==0)reject(new Error('server failed '+code+': '+errors));});});}
async function stop(){if(!server||server.exitCode!==null)return;const child=server;server=undefined;await new Promise((resolve,reject)=>{const timer=setTimeout(()=>{child.kill('SIGKILL');reject(new Error('graceful shutdown timed out'));},10000);child.once('exit',code=>{clearTimeout(timer);code===0?resolve():reject(new Error('server shutdown '+code));});child.kill('SIGINT');});}
(async()=>{
 const config=path.join(dir,'config.json'),policy=path.join(dir,'policy.json'),limits=path.join(dir,'limits.json');
 command(['fixture','EURUSD',path.join(dir,'source.json')]);command(['pack-native',path.join(dir,'source.json'),path.join(dir,'source.rsf')]);command(['corpus-create',path.join(dir,'corpus.json'),path.join(dir,'source.rsf')]);
 fs.writeFileSync(config,JSON.stringify({specification:'corpus.json',seed:42,maximum_candidates:1,queue_width:1,budget:{max_candles:10000,feature_cells:100000,max_receipts:10000},limits:{workers:1,max_parallel_candles:10000,max_parallel_feature_cells:100000,max_parallel_receipts:10000,max_new_chunks:64,lease_ms:60000}}));
 fs.writeFileSync(policy,JSON.stringify({backoff_ms:1,maximum_slots:1}));fs.writeFileSync(limits,JSON.stringify({maximum_wall_ms:15000,shutdown_grace_ms:100,maximum_output_bytes:1048576}));
 command(['corpus-search',config,db,'100']);const initial=JSON.parse(command(['registry-check',db]));assert(Number.isSafeInteger(initial.attempts));
 const add=()=>{const value=JSON.parse(command(['corpus-supervise-owned',config,policy,db,'1','0',limits]));assert.equal(value.reaped,true);assert.equal(value.stop_reason,'child-exited');return value;};
 for(let i=0;i<160;i++)add();
 const cli=kind=>{let after='--',rows=[];for(let n=0;n<20;n++){const p=JSON.parse(command([kind,db,after,'31']));assert(p.rows.length<=31);rows.push(...p.rows);if(!p.next_cursor)return rows;after=p.next_cursor;}throw Error('history did not terminate');};
 const owned=cli('corpus-owned-page'),sessions=cli('corpus-supervision-page');assert.equal(owned.length,160);assert.equal(sessions.length,160);assert(owned.every(r=>Object.hasOwn(r,'finished_at_ms')&&r.finished_at_ms!==null));assert(sessions.every(r=>Object.hasOwn(r,'finished_at_ms')&&r.finished_at_ms!==null));
 assert.equal(JSON.parse(command(['registry-check',db])).attempts,initial.attempts);
 const backup=path.join(dir,'backup.sqlite');command(['backup',db,backup]);assert.deepEqual(JSON.parse(command(['corpus-owned-page',backup,'--','128'])),JSON.parse(command(['corpus-owned-page',db,'--','128'])));
 command(['auth-init',auth],password+'\n');let base=await start();browser=await chromium.launch({headless:true,...(process.env.CHROMIUM_PATH?{executablePath:process.env.CHROMIUM_PATH}:fs.existsSync('/usr/bin/chromium')?{executablePath:'/usr/bin/chromium'}:{})});
 const context=await browser.newContext({acceptDownloads:true}),page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
 for(const route of ['owned-invocations','supervision-sessions'])assert.equal((await context.request.get(base+'/api/'+route+'?limit=32')).status(),401);
 async function login(){await page.goto(base);await page.getByLabel('Password').fill(password);await Promise.all([page.waitForNavigation(),page.getByRole('button',{name:'Sign in',exact:true}).click()]);await page.locator('#owned-cards article').first().waitFor();await page.locator('#supervision-cards article').first().waitFor();}
 await login();const session=await(await context.request.get(base+'/api/session')).json();
 for(const route of ['owned-invocations','supervision-sessions']){
  for(const query of ['limit=0','limit=129','after=bad','after='+'0'.repeat(64)])assert.equal((await context.request.get(base+'/api/'+route+'?'+query)).status(),400);
  assert.equal((await context.request.get(base+'/api/'+route+'?metrics=approved')).status(),400);
  assert.equal((await context.request.post(base+'/api/'+route,{headers:{'X-CSRF-Token':session.csrf},data:{}})).status(),405);
 }
 const defs=[{prefix:'owned',route:'owned-invocations',attribute:'data-invocation-id',ids:owned.map(r=>r.id),inspect:'Inspect process observation',export:'Export process observation'},{prefix:'supervision',route:'supervision-sessions',attribute:'data-session-id',ids:sessions.map(r=>r.id),inspect:'Inspect session',export:'Export session status'}];
 const visible=async d=>page.locator('#'+d.prefix+'-cards article').evaluateAll((nodes,attr)=>nodes.map(n=>n.getAttribute(attr)),d.attribute);
 for(const d of defs){
  let collected=[];
  for(let n=0;n<5;n++){
   await page.locator('#'+d.prefix+'-cards article').first().waitFor();const rows=await visible(d);assert.deepEqual(rows,d.ids.slice(n*32,(n+1)*32));collected.push(...rows);
   if(n<4){await page.locator('#'+d.prefix+'-older').click();await page.locator('#'+d.prefix+'-cards article['+d.attribute+'="'+d.ids[(n+1)*32]+'"]').waitFor();}
  }
  assert.deepEqual(collected,d.ids);assert(await page.locator('#'+d.prefix+'-older').isDisabled());
  const id=d.ids.at(-1),raw=await(await context.request.get(base+'/api/'+d.route+'/'+id)).text();
  await page.locator('#'+d.prefix+'-cards article['+d.attribute+'="'+id+'"]').getByRole('button',{name:d.inspect,exact:true}).click();await page.locator('#'+d.prefix+'-json').filter({hasText:id}).waitFor();
  const download=page.waitForEvent('download');await page.getByRole('button',{name:d.export,exact:true}).click();assert.equal(fs.readFileSync(await(await download).path(),'utf8'),raw);
  await page.locator('#'+d.prefix+'-newer').click();await page.locator('#'+d.prefix+'-cards article['+d.attribute+'="'+d.ids[96]+'"]').waitFor();
  await page.locator('#'+d.prefix+'-refresh').click();await page.locator('#'+d.prefix+'-cards article['+d.attribute+'="'+d.ids[96]+'"]').waitFor();assert.deepEqual(await visible(d),d.ids.slice(96,128));
  await page.locator('#'+d.prefix+'-newest').click();await page.locator('#'+d.prefix+'-cards article['+d.attribute+'="'+d.ids[0]+'"]').waitFor();
 }
 add();
 for(const d of defs){
  await page.locator('#'+d.prefix+'-older').click();await page.locator('#'+d.prefix+'-cards article['+d.attribute+'="'+d.ids[32]+'"]').waitFor();assert.deepEqual(await visible(d),d.ids.slice(32,64));
  await page.locator('#'+d.prefix+'-newest').click();await page.locator('#'+d.prefix+'-cards article').first().waitFor();
  const latest=await visible(d);assert.notEqual(latest[0],d.ids[0]);
  const pattern=new RegExp('/api/'+d.route+'\\?');let release,arrivedResolve;const arrived=new Promise(r=>arrivedResolve=r);let held=false;
  await page.route(pattern,async route=>{if(!held&&new URL(route.request().url()).searchParams.has('after')){held=true;const response=await route.fetch();release=()=>route.fulfill({response});arrivedResolve();}else await route.continue();});
  await page.locator('#'+d.prefix+'-older').click();await arrived;await page.locator('#'+d.prefix+'-refresh').click();await page.locator('#'+d.prefix+'-cards article['+d.attribute+'="'+latest[0]+'"]').waitFor();await release();await page.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));assert.deepEqual(await visible(d),latest);await page.unroute(pattern);
 }
 for(const viewport of [{width:1280,height:900},{width:390,height:844}]){await page.setViewportSize(viewport);await page.reload();await page.locator('#owned-cards article').first().waitFor();assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1));}
 await stop();base=await start();await login();
 for(const d of defs){const response=await context.request.get(base+'/api/'+d.route+'?limit=1&after='+d.ids.at(-2));assert.equal(response.status(),200);assert.equal((await response.json()).rows[0].id,d.ids.at(-1));}
 // Old corruption must be detected beyond the latest128 records, without deleting evidence.
 const old=owned.at(-1).id;const corrupt=spawnSync('python3',['-c',"import sqlite3,sys;c=sqlite3.connect(sys.argv[1]);c.execute('DROP TRIGGER owned_invocation_terminal');c.execute('UPDATE corpus_owned_invocations SET hash=? WHERE id=?',('0'*64,sys.argv[2]));c.commit()",db,old],{encoding:'utf8'});assert.equal(corrupt.status,0,corrupt.stderr);
 assert.equal((await context.request.get(base+'/api/owned-invocations?limit=32')).status(),200);assert.equal((await context.request.get(base+'/api/owned-invocations?limit=1&after='+owned.at(-2).id)).status(),500);assert.equal((await context.request.get(base+'/api/owned-invocations/'+old)).status(),500);
 const failed=spawnSync(binary,['registry-check',db],{encoding:'utf8'});assert.notEqual(failed.status,0);assert.deepEqual(errors,[]);
 console.log(JSON.stringify({browser:'Chromium',synthetic:true,actual_finished_owned_invocations:161,actual_finished_sessions:161,passed:['lifetime admission beyond128 with unchanged financial attempts','bounded CLI/authenticated keyset pages and query refusal','indexed page integrity including old corruption','desktop/mobile first/newer/older and refresh ownership','concurrent insert keyset stability','delayed inventory response refusal','exact old receipt export and restart/backup parity','no mutation or execution history endpoint']}));
})().catch(e=>{console.error(e);process.exitCode=1;}).finally(async()=>{if(browser)await browser.close();try{await stop();}catch(e){console.error(e);process.exitCode=1;}fs.rmSync(dir,{recursive:true,force:true});});
