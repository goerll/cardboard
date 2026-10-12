// Run against an isolated starter workspace; see docs/performance.md.
const pages=await (await fetch(`http://127.0.0.1:${process.env.CARDBOARD_DEBUG_PORT??9223}/json/list`)).json();
const page=pages.find(p=>p.type==='page');
if(!page) throw new Error('No CEF page');
const ws=new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve,reject)=>{ws.addEventListener('open',resolve,{once:true});ws.addEventListener('error',reject,{once:true})});
let id=0;const pending=new Map();
ws.addEventListener('message',e=>{const v=JSON.parse(e.data);if(v.id&&pending.has(v.id)){const {resolve,reject,timer}=pending.get(v.id);clearTimeout(timer);pending.delete(v.id);v.error?reject(new Error(JSON.stringify(v.error))):resolve(v.result);}});
function call(method,params={}){return new Promise((resolve,reject)=>{const request=++id;const timer=setTimeout(()=>{pending.delete(request);reject(new Error('CDP timeout'))},10000);pending.set(request,{resolve,reject,timer});ws.send(JSON.stringify({id:request,method,params}));});}
async function evaluate(expression){const v=await call('Runtime.evaluate',{expression,returnByValue:true});if(v.exceptionDetails)throw new Error(JSON.stringify(v.exceptionDetails));return v.result.value;}

const {spawn}=await import('node:child_process');
const {createInterface}=await import('node:readline');
const {fileURLToPath}=await import('node:url');
const binary=process.env.CARDBOARD_BINARY??fileURLToPath(new URL('../target/debug/cardboard',import.meta.url));
const driver=spawn(binary,['mcp']);
let requestId=0;const requests=new Map();
createInterface({input:driver.stdout}).on('line',line=>{const response=JSON.parse(line),entry=requests.get(response.id);if(entry){requests.delete(response.id);clearTimeout(entry.timer);response.error?entry.reject(Error(JSON.stringify(response.error))):entry.resolve(response.result);}});
function request(method,params={}){return new Promise((resolve,reject)=>{const id=++requestId,timer=setTimeout(()=>reject(Error(`MCP timeout: ${method}`)),10000);requests.set(id,{resolve,reject,timer});driver.stdin.write(JSON.stringify({jsonrpc:'2.0',id,method,params})+'\n');});}
await request('initialize',{protocolVersion:'2024-11-05',capabilities:{},clientInfo:{name:'cardboard-desktop-check',version:'1'}});
driver.stdin.write(JSON.stringify({jsonrpc:'2.0',method:'notifications/initialized'})+'\n');
async function mcp(name,args={}){const result=await request('tools/call',{name,arguments:args});if(result.isError)throw Error(JSON.stringify(result));return result.structuredContent;}
const assert=(ok,msg)=>{if(!ok)throw Error(msg);};
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function until(fn,label){const deadline=Date.now()+5000;while(Date.now()<deadline){if(await fn())return;await sleep(5);}throw Error('Timeout: '+label);}
async function rect(id='node-12'){return evaluate(`(()=>{const e=[...document.querySelectorAll('[data-cardboard-id]')].find(n=>n.dataset.cardboardId===${JSON.stringify(id)}),r=e.getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height,zoom:window.__cardboard.zoom};})()`);}
async function input(type,x,y,shift=false){return call('Input.dispatchMouseEvent',{type,x,y,button:type==='mouseMoved'?'none':'left',buttons:type==='mouseReleased'?0:1,clickCount:type==='mouseMoved'?0:1,modifiers:shift?8:0});}
async function sync(){const r=(await mcp('get_basic_info')).revision;await until(()=>evaluate(`window.__cardboard.revision===${r}`),'browser revision');return r;}
async function undo(){await mcp('undo');await sync();}
async function select(id='node-12'){const r=await rect(id);await input('mousePressed',r.x+5,r.y+5);await input('mouseReleased',r.x+5,r.y+5);await until(async()=> (await mcp('get_selection')).nodes[0]?.id===id,'selection');await sleep(30);}
async function handlePoint(handle){return evaluate(`(()=>{const r=document.querySelector('[data-handle=${handle}]').getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})()`);}
try{
 await sync();const original=(await mcp('get_html',{nodeId:'node-12'}));
 const timeOrigin=await evaluate('performance.timeOrigin');
 await evaluate("window.__testHeading=document.querySelector('h1')");
 const editDomMs=[];
 for(let i=0;i<8;i++){
  const font=(58+i)+'px',start=performance.now();
  await mcp('update_styles',{updates:[{nodeIds:['node-12'],styles:{'font-size':font}}]});
  await until(()=>evaluate(`document.querySelector('h1').style.fontSize===${JSON.stringify(font)}`),'style patch');
  editDomMs.push(Math.round(performance.now()-start));
 }
 assert(await evaluate('window.__testHeading===document.querySelector("h1") && performance.timeOrigin==='+timeOrigin),'style edits preserve DOM identity and avoid navigation');
 for(let i=0;i<8;i++)await undo();
 console.log(JSON.stringify({editDomMs,navigationOccurred:false}));
 await select();
 const before=await rect(), rev=(await mcp('get_basic_info')).revision;
 await input('mousePressed',before.x+20,before.y+15);
 await input('mouseMoved',before.x+20+32*before.zoom,before.y+15+20*before.zoom);
 const preview=await rect();
 assert(Math.abs(preview.x-before.x-32*before.zoom)<1,'drag preview X');assert(Math.abs(preview.y-before.y-20*before.zoom)<1,'drag preview Y');
 assert((await mcp('get_basic_info')).revision===rev,'preview must not save');
 await input('mouseReleased',before.x+20+32*before.zoom,before.y+15+20*before.zoom);
 await until(async()=> (await mcp('get_basic_info')).revision===rev+1,'one drag commit');await sync();
 assert((await mcp('get_styles',{nodeIds:['node-12']})).styles['node-12'].translate==='32px 20px','persisted drag');
 await undo();assert(JSON.stringify(await mcp('get_html',{nodeId:'node-12'}))===JSON.stringify(original),'undo exact HTML');
 // Resize from each of the 8 handles. Opposite bounds stay fixed for west/north handles.
 for(const handle of ['e','s','se','w','n','nw','ne','sw']){
  await select();const b=await rect(),p=await handlePoint(handle),revision=(await mcp('get_basic_info')).revision;
  const dx=handle.includes('w')?-20:handle.includes('e')?20:0,dy=handle.includes('n')?-15:handle.includes('s')?15:0;
  await input('mousePressed',p.x,p.y);await input('mouseMoved',p.x+dx*b.zoom,p.y+dy*b.zoom);
  const r=await rect();assert(Math.abs(r.width-b.width-(dx?20*b.zoom:0))<2,handle+' width');assert(Math.abs(r.height-b.height-(dy?15*b.zoom:0))<2,handle+' height');
  if(handle.includes('w'))assert(Math.abs(r.x+r.width-b.x-b.width)<2,handle+' opposite X');
  if(handle.includes('n'))assert(Math.abs(r.y+r.height-b.y-b.height)<2,handle+' opposite Y');
  await input('mouseReleased',p.x+dx*b.zoom,p.y+dy*b.zoom);await until(async()=> (await mcp('get_basic_info')).revision===revision+1,'resize commit '+handle);await sync();await undo();
 }
 // Shift constrains movement; Escape cancels a preview with no undo entry.
 await select();const b=await rect(),revision=(await mcp('get_basic_info')).revision;
 await input('mousePressed',b.x+20,b.y+15);await input('mouseMoved',b.x+70,b.y+25,true);
 assert(Math.abs((await rect()).y-b.y)<1,'shift axis constraint');
 await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
 await input('mouseReleased',b.x+70,b.y+25);await sleep(60);
 assert((await mcp('get_basic_info')).revision===revision,'cancel must not commit');assert(Math.abs((await rect()).x-b.x)<1,'cancel restores preview');
 // An agent revision cancels an active drag and remains authoritative.
 await input('mousePressed',b.x+20,b.y+15);await input('mouseMoved',b.x+80,b.y+15);
 await mcp('update_styles',{updates:[{nodeIds:['node-12'],styles:{color:'#123456'}}]});await sync();
 await input('mouseReleased',b.x+80,b.y+15);await sleep(60);
 const styles=(await mcp('get_styles',{nodeIds:['node-12']})).styles['node-12'];assert(!styles.translate&&styles.color==='#123456','agent edit wins stale gesture');await undo();
 assert(JSON.stringify(await mcp('get_html',{nodeId:'node-12'}))===JSON.stringify(original),'all gestures restore exact HTML');
 console.log('Passed: live drag, all 8 resize handles, one commit per gesture, undo, Shift constraint, Escape, concurrent MCP edit');
}finally{driver.stdin.end();ws.close();}
