'use strict';
// Development-only integration against the actual Rust service and real SQLite fixture state.
const {chromium}=require('playwright');
const assert=require('node:assert/strict');
const fs=require('node:fs');const os=require('node:os');const path=require('node:path');
const {spawn,spawnSync}=require('node:child_process');const readline=require('node:readline');
const root=path.resolve(__dirname,'..');const binary=process.env.FACTORY_BIN||path.join(root,'target/debug/rust-strategy-factory');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'factory-browser-'));const db=path.join(dir,'state.sqlite'),auth=path.join(dir,'auth.json');
const password='fixture-only-password-not-for-production';let server,browser;
function command(args,input){const r=spawnSync(binary,args,{encoding:'utf8',input});assert.equal(r.status,0,r.stderr);return r.stdout;}
async function start(){server=spawn(binary,['serve',db,auth,'0'],{stdio:['ignore','pipe','pipe']});let errors='';server.stderr.on('data',d=>errors+=d);return new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(new Error('server readiness timed out: '+errors)),15000);const lines=readline.createInterface({input:server.stdout});lines.on('line',line=>{let r;try{r=JSON.parse(line);}catch{return;}if(r.listening_port){clearTimeout(timer);resolve('http://127.0.0.1:'+r.listening_port);}});server.once('error',e=>{clearTimeout(timer);reject(e);});server.once('exit',code=>{clearTimeout(timer);if(code!==0)reject(new Error('server failed '+code+': '+errors));});});}
async function stop(){if(!server||server.exitCode!==null)return;const child=server;server=undefined;await new Promise((resolve,reject)=>{const timer=setTimeout(()=>{child.kill('SIGKILL');reject(new Error('graceful shutdown timed out'));},10000);child.once('exit',code=>{clearTimeout(timer);code===0?resolve():reject(new Error('server shutdown '+code));});child.kill('SIGINT');});}
(async()=>{
 command(['fixture','EURUSD',path.join(dir,'input.json')]);fs.writeFileSync(path.join(dir,'research.json'),JSON.stringify({inputs:[{dataset:'input.json'}]}));
 const runOutput=command(['research',path.join(dir,'research.json'),db,'10']);const runId=JSON.parse(runOutput.split('\n')[0]).run_id;
 command(['auth-init',auth],password+'\n');assert(!fs.readFileSync(auth,'utf8').includes(password));assert.equal(fs.statSync(auth).mode&0o777,0o600);
 const originalAuth=fs.readFileSync(auth,'utf8');assert.notEqual(spawnSync(binary,['auth-init',auth],{input:password+'\n'}).status,0);assert.equal(fs.readFileSync(auth,'utf8'),originalAuth);
 fs.chmodSync(auth,0o644);assert.notEqual(spawnSync(binary,['serve',db,auth,'0'],{timeout:3000}).status,0);fs.chmodSync(auth,0o600);
 let base=await start();browser=await chromium.launch({headless:true,...(process.env.CHROMIUM_PATH?{executablePath:process.env.CHROMIUM_PATH}:fs.existsSync('/usr/bin/chromium')?{executablePath:'/usr/bin/chromium'}:{})});
 const context=await browser.newContext({viewport:{width:1280,height:900},acceptDownloads:true});const page=await context.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
 assert.equal((await context.request.get(base+'/api/overview')).status(),401);
 assert.equal((await context.request.get(base+'/api/health',{headers:{Host:'untrusted.example'}})).status(),403);
 assert.equal((await context.request.post(base+'/api/login',{data:{password:'x'.repeat(9000)}})).status(),413);
 await page.goto(base);await page.getByLabel('Password').fill('wrong-password');await page.getByRole('button',{name:'Sign in',exact:true}).click();await page.getByRole('alert').filter({hasText:'invalid password'}).waitFor();
 await page.getByLabel('Password').fill(password);await Promise.all([page.waitForNavigation(),page.getByRole('button',{name:'Sign in',exact:true}).click()]);await page.getByRole('heading',{name:'Recent experiments'}).waitFor();await page.locator('#results tr').first().waitFor();
 const cookies=await context.cookies();assert(cookies.some(c=>c.name==='factory_sid'&&c.httpOnly&&c.sameSite==='Strict'));
 const session=await (await context.request.get(base+'/api/session')).json();
 assert.equal((await context.request.post(base+'/api/runs/'+runId+'/control',{data:{state:'paused'}})).status(),403);
 assert.equal((await context.request.post(base+'/api/runs/'+runId+'/control',{headers:{'X-CSRF-Token':session.csrf,Origin:'https://untrusted.example'},data:{state:'paused'}})).status(),403);
 assert.equal((await context.request.post(base+'/api/experiments',{headers:{'X-CSRF-Token':session.csrf},data:{win_rate:100}})).status(),405);
 await page.getByRole('button',{name:'Pause',exact:true}).first().click();await page.locator('#runs').filter({hasText:'paused'}).waitFor();
 const overview=await (await context.request.get(base+'/api/overview')).json();assert.equal(overview.runs.find(r=>r.id===runId).state,'paused');assert.equal(overview.counts.completed,3);
 await page.getByLabel('Instrument',{exact:true}).selectOption('EURUSD');const completed=page.locator('#results tr').filter({hasText:'completed'}).first();await completed.getByRole('button').click();await page.locator('#detail:not([hidden])').waitFor();assert((await page.locator('#detail-label').textContent()).includes('SYNTHETIC_TEST_ONLY'));assert(await page.locator('#trades tr').count()>0);assert.equal(await page.locator('svg').count(),1);
 const download=page.waitForEvent('download');await page.getByRole('button',{name:'Export full receipt'}).click();const file=await download;assert.match(file.suggestedFilename(),/^[a-f0-9]{64}\.json$/);const receipt=JSON.parse(fs.readFileSync(await file.path(),'utf8'));assert(receipt.result.tick_replay.trades.length>0);assert.match(receipt.result_hash,/^[a-f0-9]{64}$/);assert.equal(receipt.result.instrument_profile.symbol,'EURUSD');assert.equal(receipt.result.cost_profile.spread_floor_pips,2);assert.equal(receipt.numeric_training_assessment.pooled.resolved,receipt.result.tick_replay.trades.length);assert.notEqual(receipt.numeric_training_assessment.status,'VALIDATED');assert.equal(await page.locator('#trades tr').first().locator('td').nth(3).textContent(),(receipt.result.tick_replay.trades[0].entry/100000).toFixed(5));
 fs.mkdirSync(path.join(root,'state/browser'),{recursive:true});await page.screenshot({path:path.join(root,'state/browser/desktop.png'),fullPage:true});
 await page.setViewportSize({width:390,height:844});await page.goto(base);await page.locator('#results tr').first().waitFor();assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1));await page.screenshot({path:path.join(root,'state/browser/mobile.png'),fullPage:true});
 await stop();base=await start();assert.equal((await context.request.get(base+'/api/overview')).status(),401);await page.goto(base);await page.getByLabel('Password').fill(password);await Promise.all([page.waitForNavigation(),page.getByRole('button',{name:'Sign in',exact:true}).click()]);await page.locator('#runs').filter({hasText:'paused'}).waitFor();
 await page.getByRole('button',{name:'Sign out'}).click();await page.getByRole('button',{name:'Sign in',exact:true}).waitFor();assert.equal((await context.request.get(base+'/api/overview')).status(),401);
 for(let i=0;i<9;i++){const response=await context.request.post(base+'/api/login',{data:{password:'invalid'}});if(i===8)assert.equal(response.status(),429);}
 assert.deepEqual(errors,[]);console.log(JSON.stringify({browser:'Chromium',passed:['authentication','body limit','HttpOnly/SameSite session','CSRF and origin rejection','metrics write rejection','persisted pause control','strategy/trades/equity','raw receipt export','desktop/mobile','restart persistence','logout','login rate limit','auth file permissions/no overwrite','host rejection','numeric diagnostics'],synthetic:true}));
})().catch(e=>{console.error(e);process.exitCode=1;}).finally(async()=>{if(browser)await browser.close();try{await stop();}catch(e){console.error(e);process.exitCode=1;}fs.rmSync(dir,{recursive:true,force:true});});
