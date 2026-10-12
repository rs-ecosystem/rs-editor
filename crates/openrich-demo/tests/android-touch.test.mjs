import test from 'node:test';
import assert from 'node:assert/strict';
const endpoint=process.env.OPENRICH_ANDROID_CDP, pageUrl=process.env.OPENRICH_ANDROID_TEST_URL;
test('Android touch double tap opens editor precision handles and menu', {skip:!endpoint||!pageUrl}, async()=>{
  const pages=await(await fetch(`${endpoint}/json`)).json(),target=pages.find(page=>page.url===pageUrl);
  assert.ok(target,'isolated test page must be open');
  const socket=new WebSocket(target.webSocketDebuggerUrl);await new Promise(resolve=>socket.addEventListener('open',resolve,{once:true}));
  let sequence=0;const pending=new Map();
  socket.addEventListener('message',event=>{const message=JSON.parse(event.data),request=pending.get(message.id);if(!request)return;pending.delete(message.id);clearTimeout(request.timer);if(message.error||message.result?.exceptionDetails)request.reject(new Error(JSON.stringify(message.error||message.result.exceptionDetails)));else request.resolve(message.result);});
  const send=(method,params={})=>new Promise((resolve,reject)=>{const id=++sequence,timer=setTimeout(()=>reject(new Error('CDP timeout')),15000);pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method,params}));});
  const evaluate=async expression=>(await send('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true})).result.value;
  const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const point=()=>evaluate(`(()=>{const editor=document.querySelector('#editor');editor.scrollIntoView({block:'center'});const node=editor.querySelector('.text-run').firstChild,range=document.createRange();range.setStart(node,2);range.setEnd(node,3);const rect=range.getBoundingClientRect();return {x:rect.x+rect.width/2,y:rect.y+rect.height/2};})()`);
  const tap=async position=>{await send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[position]});await delay(30);await send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});};
  try {
    await send('Page.reload',{ignoreCache:true});
    for(let attempt=0;attempt<100;attempt++){if(await evaluate('!!document.querySelector("#editor .text-run")'))break;await delay(50);}
    await evaluate('document.fonts.ready.then(()=>true)');
    await tap(await point());await delay(600);
    await evaluate(`(()=>{window.__openrichTouchTest=[];const editor=document.querySelector('#editor');for(const type of ['pointerdown','pointerup','dblclick','contextmenu'])editor.addEventListener(type,event=>queueMicrotask(()=>window.__openrichTouchTest.push({type,prevented:event.defaultPrevented,trusted:event.isTrusted})));return true;})()`);
    const location=await point();await tap(location);await delay(70);await tap(location);await delay(600);
    const result=await evaluate(`({selection:getSelection().toString(),trace:window.__openrichTouchTest,customHandles:[...document.querySelectorAll('.selection-handle')].some(handle=>!handle.hidden)})`);
    assert.ok(result.selection.length>0,JSON.stringify(result));
    assert.equal(result.customHandles,true,JSON.stringify(result));
    assert.ok(result.trace.some(event=>event.type==='pointerdown'&&event.trusted),JSON.stringify(result));
    assert.ok(result.trace.some(event=>event.type==='contextmenu'&&event.trusted),JSON.stringify(result));
    assert.ok(result.trace.filter(event=>event.type === "pointerdown" || event.type === "pointerup").every(event=>!event.prevented),JSON.stringify(result));
    assert.ok(result.trace.some(event=>event.type === "contextmenu" && event.prevented),JSON.stringify(result));
  } finally {socket.close();}
});
