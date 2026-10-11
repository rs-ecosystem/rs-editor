import test from 'node:test';
import assert from 'node:assert/strict';
const endpoint=process.env.OPENRICH_ANDROID_CDP;
const testUrl=process.env.OPENRICH_ANDROID_TEST_URL;
test('Android adapter keeps latest caret through no-op composition and delayed commit', {skip:!endpoint||!testUrl}, async()=>{
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
  const fixture={schema_version:2,blocks:[{type:'paragraph',id:'98001',content:[{type:'text',text:'Selamat datang',marks:[]}]}]};
  const original=await(await fetch(`${testUrl}api/state`)).json();
  try {
    const loaded=await fetch(`${testUrl}api/load`,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(fixture)});assert.equal(loaded.status,200);
    await send('Page.reload',{ignoreCache:true});
    for(let attempt=0;attempt<100;attempt++){if(await evaluate('document.querySelector("#editor")?.textContent === "Selamat datang"'))break;await new Promise(resolve=>setTimeout(resolve,50));}
    const result=await evaluate(`(async()=>{
      await document.fonts.ready;
      const editor=document.querySelector('#editor');editor.focus({preventScroll:true});
      const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
      const move=offset=>{const node=editor.querySelector('.text-run').firstChild;getSelection().setBaseAndExtent(node,offset,node,offset);document.dispatchEvent(new Event('selectionchange'));};
      const offset=()=>{const selection=getSelection(),range=document.createRange();range.selectNodeContents(editor.querySelector('[data-block="0"]'));range.setEnd(selection.focusNode,selection.focusOffset);return range.toString().length;};
      const originalFetch=window.fetch;
      window.fetch=async(...args)=>{if(String(args[0]).includes('/api/action'))await delay(80);return originalFetch(...args);};
      try {
        move(3);editor.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true,data:''}));
        move(10);editor.dispatchEvent(new CompositionEvent('compositionend',{bubbles:true,data:''}));
        const immediateNoop=offset();await delay(350);const settledNoop=offset();
        move(3);editor.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true,data:''}));
        const node=editor.querySelector('.text-run').firstChild;node.insertData(3,'x');move(4);
        editor.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertCompositionText',data:'x',isComposing:true}));
        editor.dispatchEvent(new CompositionEvent('compositionend',{bubbles:true,data:'x'}));
        move(11);const immediateCommit=offset();await delay(450);const settledCommit=offset();
        return {immediateNoop,settledNoop,immediateCommit,settledCommit,text:editor.textContent};
      } finally {window.fetch=originalFetch;}
    })()`);
    assert.deepEqual(result,{immediateNoop:10,settledNoop:10,immediateCommit:11,settledCommit:11,text:'Selxamat datang'});
  } finally {
    await fetch(`${testUrl}api/load`,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(original.document)});
    socket.close();
  }
});
