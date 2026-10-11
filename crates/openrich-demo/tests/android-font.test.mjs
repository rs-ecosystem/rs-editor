import test from 'node:test';
import assert from 'node:assert/strict';
const endpoint=process.env.OPENRICH_ANDROID_CDP;
const pageUrl=process.env.OPENRICH_ANDROID_URL;
test('Android native Latin endpoints match Skia with shared regular/bold fonts', {skip:!endpoint||!pageUrl}, async () => {
  const pages=await (await fetch(`${endpoint}/json`)).json();
  const target=pages.find(page=>page.url===pageUrl);
  assert.ok(target,'demo page must be open on Android');
  const socket=new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve,reject)=>{socket.addEventListener('open',resolve,{once:true});socket.addEventListener('error',reject,{once:true});});
  const expression=`(async()=>{
    await document.fonts.ready;
    const editor=document.querySelector('#editor'), canvas=document.querySelector('#skiaCanvas').getBoundingClientRect();
    const response=await fetch('/api/scene?width='+document.querySelector('#surface').clientWidth+'&scale='+devicePixelRatio+'&format=png');
    const bytes=await response.arrayBuffer(), length=new DataView(bytes).getUint32(0,true);
    const metadata=JSON.parse(new TextDecoder().decode(new Uint8Array(bytes,4,length)));
    const {prepareScene,caretRect}=await import('/interaction.mjs');const scene=prepareScene(metadata);
    const walker=document.createTreeWalker(editor,NodeFilter.SHOW_TEXT), results=[];let node;
    while((node=walker.nextNode())&&results.length<20){
      const block=node.parentElement.closest('[data-block]');if(!block)continue;
      for(const match of node.textContent.matchAll(/[A-Za-z]+/g)){
        const range=document.createRange();range.setStart(node,match.index);range.setEnd(node,match.index+match[0].length);
        if(range.getClientRects().length!==1)continue;
        const rect=range.getBoundingClientRect();const before=document.createRange();before.selectNodeContents(block);before.setEnd(node,match.index+match[0].length);
        const offset=[...new Intl.Segmenter(undefined,{granularity:'grapheme'}).segment(before.toString())].length;
        const caret=caretRect(scene,{block_index:Number(block.dataset.block),offset,affinity:'upstream'});
        if(caret)results.push({word:match[0],weight:getComputedStyle(node.parentElement).fontWeight,dx:rect.right-canvas.left-caret.x,dy:rect.bottom-canvas.top-caret.y-caret.h});
      }
    }return results;
  })()`;
  try {
    const result=await new Promise((resolve,reject)=>{
      const timeout=setTimeout(()=>reject(new Error('Android geometry request timed out')),15000);
      socket.addEventListener('message',event=>{const message=JSON.parse(event.data);if(message.id!==1)return;clearTimeout(timeout);if(message.result?.exceptionDetails)reject(new Error(JSON.stringify(message.result.exceptionDetails)));else resolve(message.result.result.value);});
      socket.send(JSON.stringify({id:1,method:'Runtime.evaluate',params:{expression,returnByValue:true,awaitPromise:true}}));
    });
    assert.ok(result.length>0,'at least one Latin word must be present');
    for(const endpoint of result){assert.ok(Math.abs(endpoint.dx)<0.5,`horizontal mismatch ${JSON.stringify(endpoint)}`);assert.ok(Math.abs(endpoint.dy)<1.25,`vertical mismatch ${JSON.stringify(endpoint)}`);}
  } finally {socket.close();}
});
