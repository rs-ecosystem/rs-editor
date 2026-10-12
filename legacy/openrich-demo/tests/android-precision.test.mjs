import test from 'node:test';
import assert from 'node:assert/strict';
const endpoint=process.env.OPENRICH_ANDROID_CDP;
const testUrl=process.env.OPENRICH_ANDROID_TEST_URL;
test('Android precision handle extends heading selection directly into list characters', {skip:!endpoint||!testUrl}, async()=>{
  const pages=await(await fetch(`${endpoint}/json`)).json();
  const target=pages.find(page=>page.url===testUrl);
  assert.ok(target,'isolated test page must be open');
  const socket=new WebSocket(target.webSocketDebuggerUrl);
  await new Promise(resolve=>socket.addEventListener('open',resolve,{once:true}));
  let sequence=0;
  const pending=new Map();
  socket.addEventListener('message',event=>{const message=JSON.parse(event.data), request=pending.get(message.id);if(!request)return;pending.delete(message.id);clearTimeout(request.timer);if(message.error||message.result?.exceptionDetails)request.reject(new Error(JSON.stringify(message.error||message.result.exceptionDetails)));else request.resolve(message.result);});
  const send=(method,params={})=>new Promise((resolve,reject)=>{const id=++sequence,timer=setTimeout(()=>{pending.delete(id);reject(new Error('CDP timeout'));},15000);pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method,params}));});
  const evaluate=async expression=>(await send('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true})).result.value;
  const fixture={schema_version:2,blocks:[
    {type:'heading',id:'99001',level:1,content:[{type:'text',text:'Selamat datang test 729391',marks:[]}]},
    {type:'formatted',id:'99002',format:{heading_level:null,alignment:'left',list:'ordered',checked:false},content:[{type:'text',text:'skskCTskskigjkskdhhdishhskshh',marks:[]}]}
  ]};
  const original=await(await fetch(`${testUrl}api/state`)).json();
  const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  try {
    await fetch(`${testUrl}api/load`,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(fixture)});
    await send('Page.reload',{ignoreCache:true});
    for(let i=0;i<100;i++){if(await evaluate('document.querySelector("#editor")?.textContent === "Selamat datang test 729391skskCTskskigjkskdhhdishhskshh" && document.querySelector("#loading").classList.contains("hidden")'))break;await delay(50);}
    await evaluate(`(async()=>{
      await document.fonts.ready;const editor=document.querySelector('#editor');editor.scrollIntoView({block:'center'});
      editor.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true,pointerType:'touch',button:0}));
      const node=editor.querySelector('[data-block="0"] .text-run').firstChild,offset=node.textContent.indexOf('729391');
      getSelection().setBaseAndExtent(node,offset,node,offset+6);
      editor.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true}));
      return true;
    })()`);
    await delay(250);
    const points=await evaluate(`(()=>{
      const handle=document.querySelector('#selectionEnd'),h=handle.getBoundingClientRect(),s=getSelection(),r=document.createRange();
      r.setStart(s.focusNode,s.focusOffset);r.collapse(true);const c=r.getBoundingClientRect();
      const x=h.x+h.width/2,y=h.y+10,dx=x-c.x,dy=y-c.y-c.height/2;
      const node=document.querySelector('[data-block="1"] .text-run').firstChild;
      const targets=[1,3,6].map(offset=>{r.setStart(node,offset);r.collapse(true);const b=r.getBoundingClientRect();return {x:b.x+dx-visualViewport.offsetLeft,y:b.y+b.height/2+dy-visualViewport.offsetTop};});
      window.__precisionEvents=[];handle.addEventListener('pointerdown',e=>window.__precisionEvents.push({trusted:e.isTrusted,type:e.type}));
      return {start:{x:x-visualViewport.offsetLeft,y:y-visualViewport.offsetTop},targets,hidden:handle.hidden,selected:s.toString()};
    })()`);
    assert.equal(points.hidden,false);assert.equal(points.selected,'729391');
    await send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[points.start]});await delay(40);
    const offsets=[];
    for(const target of points.targets){
      await send('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[target]});await delay(80);
      offsets.push(await evaluate('({block:getSelection().focusNode.parentElement.closest("[data-block]").dataset.block,offset:getSelection().focusOffset})'));
    }
    await send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await delay(100);
    assert.deepEqual(offsets,[{block:'1',offset:1},{block:'1',offset:3},{block:'1',offset:6}]);
    const result=await evaluate('({text:getSelection().toString(),events:window.__precisionEvents,handles:[...document.querySelectorAll(".selection-handle")].filter(e=>!e.hidden).length,menu:!document.querySelector("#selectionMenu").hidden})');
    assert.ok(result.events.some(e=>e.trusted));assert.equal(result.handles,2);assert.equal(result.menu,true);
    assert.ok(result.text.startsWith('729391'));assert.ok(result.text.endsWith('skskCT'));
    assert.deepEqual((await(await fetch(`${testUrl}api/state`)).json()).document,fixture,'drag must not modify document');
  } finally {
    await fetch(`${testUrl}api/load`,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(original.document)});
    socket.close();
  }
});
