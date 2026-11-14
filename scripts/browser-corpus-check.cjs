'use strict';
// Development-only integration against the actual Rust service and real SQLite fixture state.
const {chromium}=require('playwright');
const assert=require('node:assert/strict');
const fs=require('node:fs');const os=require('node:os');const path=require('node:path');
const {spawn,spawnSync}=require('node:child_process');const readline=require('node:readline');
const root=path.resolve(__dirname,'..');const binary=process.env.FACTORY_BIN||path.join(root,'target/debug/rust-strategy-factory');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'factory-corpus-browser-'));const db=path.join(dir,'state.sqlite'),auth=path.join(dir,'auth.json');
const catalog=path.join(dir,'catalog.sqlite');let catalogConfigured=false;
const password='fixture-only-password-not-for-production';let server,browser;
function command(args,input){const r=spawnSync(binary,args,{encoding:'utf8',input});assert.equal(r.status,0,r.stderr);return r.stdout;}
async function start(){server=spawn(binary,['serve',db,auth,'0',...(catalogConfigured?[catalog]:[])],{stdio:['ignore','pipe','pipe']});let errors='';server.stderr.on('data',d=>errors+=d);return new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(new Error('server readiness timed out: '+errors)),15000);const lines=readline.createInterface({input:server.stdout});lines.on('line',line=>{let r;try{r=JSON.parse(line);}catch{return;}if(r.listening_port){clearTimeout(timer);resolve('http://127.0.0.1:'+r.listening_port);}});server.once('error',e=>{clearTimeout(timer);reject(e);});server.once('exit',code=>{clearTimeout(timer);if(code!==0)reject(new Error('server failed '+code+': '+errors));});});}
async function stop(){if(!server||server.exitCode!==null)return;const child=server;server=undefined;await new Promise((resolve,reject)=>{const timer=setTimeout(()=>{child.kill('SIGKILL');reject(new Error('graceful shutdown timed out'));},10000);child.once('exit',code=>{clearTimeout(timer);code===0?resolve():reject(new Error('server shutdown '+code));});child.kill('SIGINT');});}
(async()=>{
 const profile=JSON.parse(command(['profiles'])).find(p=>p.symbol==='EURUSD');fs.writeFileSync(path.join(dir,'profile.json'),JSON.stringify(profile));
 const epoch=Date.parse('2025-01-01T00:00:00Z');
 for(let chunk=0;chunk<2;chunk++){
  const rows=[];for(let i=0;i<4;i++)rows.push([epoch+(chunk*4+i)*60000,(18446744073709551600n+BigInt(chunk*4+i)).toString(),'1.00000','1.00010'].join(','));
  fs.writeFileSync(path.join(dir,chunk+'.csv'),'timestamp_ms,sequence,bid,ask\n'+rows.join('\n')+'\n');
  command(['import',path.join(dir,chunk+'.csv'),path.join(dir,'profile.json'),'SYNTHETIC_TEST_ONLY corpus browser',path.join(dir,chunk+'.json'),'synthetic']);command(['pack-native',path.join(dir,chunk+'.json'),path.join(dir,chunk+'.rsf')]);
 }
 const spec=path.join(dir,'corpus.json'),strategy=path.join(dir,'strategy.json');command(['corpus-create',spec,path.join(dir,'0.rsf'),path.join(dir,'1.rsf')]);fs.writeFileSync(strategy,JSON.stringify({timeframe:'M1',side:'Long',stop_ticks:100,rule:{AboveMean:{window:2}}}));
 const pending=JSON.parse(command(['corpus-step',spec,strategy,db,'100','10000','1000','1','60000']));const id=pending.Pending.experiment_id;assert.equal(pending.Pending.cursor,1);
 command(['auth-init',auth],password+'\n');let base=await start();browser=await chromium.launch({headless:true,...(process.env.CHROMIUM_PATH?{executablePath:process.env.CHROMIUM_PATH}:fs.existsSync('/usr/bin/chromium')?{executablePath:'/usr/bin/chromium'}:{})});const context=await browser.newContext({acceptDownloads:true});const page=await context.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
 assert.equal((await context.request.get(base+'/api/corpus/'+id+'/status')).status(),401);assert.equal((await context.request.post(base+'/api/corpus/'+id+'/control',{data:{state:'paused'}})).status(),401);
 async function login(){await page.goto(base);await page.getByLabel('Password').fill(password);await Promise.all([page.waitForNavigation(),page.getByRole('button',{name:'Sign in',exact:true}).click()]);await page.getByRole('button',{name:'Recovery',exact:true}).click();await page.locator('#corpus-status').filter({hasText:'checkpoint 1'}).waitFor();}
 await login();let session=await(await context.request.get(base+'/api/session')).json();
 assert.equal((await context.request.post(base+'/api/corpus/'+id+'/control',{data:{state:'paused'}})).status(),403);assert.equal((await context.request.post(base+'/api/corpus/'+id+'/control',{headers:{'X-CSRF-Token':session.csrf,Origin:'https://untrusted.example'},data:{state:'paused'}})).status(),403);
 assert.equal((await context.request.get(base+'/api/corpus/bad/status')).status(),400);assert.equal((await context.request.get(base+'/api/corpus/'+'0'.repeat(64)+'/status')).status(),404);
 const before=await(await context.request.get(base+'/api/corpus/'+id+'/status')).text();assert(!before.includes('token'));assert(!before.includes('account'));const hash=JSON.parse(before).checkpoint_hash;
 await page.getByRole('button',{name:'Pause corpus',exact:true}).click();await page.locator('#corpus-status').filter({hasText:'paused'}).waitFor();assert(await page.getByRole('button',{name:'Pause corpus',exact:true}).isDisabled());
 for(const viewport of [{width:1280,height:900},{width:390,height:844}]){await page.setViewportSize(viewport);await page.reload();await page.getByRole('button',{name:'Recovery',exact:true}).click();await page.locator('#corpus-status').filter({hasText:'paused'}).waitFor();assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1));}
 const download=page.waitForEvent('download');await page.getByRole('button',{name:'Export recovery status',exact:true}).click();const file=await download;const exported=JSON.parse(fs.readFileSync(await file.path(),'utf8'));assert.equal(exported.checkpoint_hash,hash);assert.equal(exported.control,'paused');assert.equal(exported.attempt,'1');
 await stop();base=await start();await login();await page.locator('#corpus-status').filter({hasText:'paused'}).waitFor();await page.getByRole('button',{name:'Resume corpus',exact:true}).click();await page.locator('#corpus-status').filter({hasText:'running'}).waitFor();await page.getByRole('button',{name:'Stop corpus permanently',exact:true}).click();await page.locator('#corpus-status').filter({hasText:'stopped'}).waitFor();assert(await page.getByRole('button',{name:'Resume corpus',exact:true}).isDisabled());
 session=await(await context.request.get(base+'/api/session')).json();assert.equal((await context.request.post(base+'/api/corpus/'+id+'/control',{headers:{'X-CSRF-Token':session.csrf},data:{state:'running'}})).status(),400);
 const held=JSON.parse(command(['corpus-step',spec,strategy,db,'100','10000','1000','1','60000']));assert.equal(held.Held.state,'stopped');assert.equal(JSON.parse(command(['corpus-status',db,id])).checkpoint_hash,hash);
 assert.deepEqual(errors,[]);console.log(JSON.stringify({browser:'Chromium',passed:['authenticated corpus status and controls','CSRF/origin refusal','pause/resume/irreversible stop','checkpoint metadata redaction and exact export','desktop/mobile recovery controls','restart persistence','stopped CLI held without advancing checkpoint'],synthetic:true}));
})().catch(e=>{console.error(e);process.exitCode=1;}).finally(async()=>{if(browser)await browser.close();try{await stop();}catch(e){console.error(e);process.exitCode=1;}fs.rmSync(dir,{recursive:true,force:true});});
