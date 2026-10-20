import test from 'node:test';
import assert from 'node:assert/strict';
const base = process.env.OPENRICH_DEMO_URL;
const caret = (offset,affinity='downstream') => ({anchor:{block_index:0,offset,affinity},focus:{block_index:0,offset,affinity}});
test('mobile composition, formatting, history, scene and stale-client API regressions', {skip:!base}, async () => {
  let state;
  const action = async (command, options={}) => {
    const response = await fetch(`${base}/api/action`,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({command,expected_document:state.document,...options})});
    assert.equal(response.status,200,await response.clone().text());
    state=await response.json();return state;
  };
  const original=await (await fetch(`${base}/api/state`)).json();
  try {
    const response=await fetch(`${base}/api/load`,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({schema_version:2,blocks:[{type:'paragraph',id:'99001',content:[]}]})});
    state=await response.json();
    await action('toggle_mark',{mark:'bold',selection:caret(0)});
    await action('composition_begin',{selection:caret(0,'upstream')});
    await action('composition_update',{text:'Selamat'});
    await action('composition_replace',{text:'Selamat',selection:caret(0)});
    assert.equal(state.document.blocks[0].content[0].text,'Selamat');
    assert.deepEqual(state.document.blocks[0].content[0].marks,[{type:'bold'}]);
    await action('composition_replace',{text:'Selamat',selection:caret(0)});
    assert.equal(state.document.blocks[0].content[0].text,'Selamat');
    await action('undo');assert.equal(state.document.blocks[0].content.length,0);
    await action('redo');assert.equal(state.document.blocks[0].content[0].text,'Selamat');
    await action('composition_begin',{selection:caret(7)});
    await action('composition_cancel');
    assert.equal(state.document.blocks[0].content[0].text,'Selamat');
    await action('set_block_format',{format:{list:'checkbox',checked:true,alignment:'center'},selection:caret(7)});
    const sceneResponse=await fetch(`${base}/api/scene?width=320&scale=2`);
    assert.equal(sceneResponse.status,200);
    const bytes=await sceneResponse.arrayBuffer();
    const len=new DataView(bytes).getUint32(0,true);
    const scene=JSON.parse(new TextDecoder().decode(new Uint8Array(bytes,4,len)));
    assert.equal(scene.blocks[0].checkbox.w,32);
    assert.deepEqual(scene.document,state.document);
    const previewDocument=structuredClone(state.document);
    previewDocument.blocks[0].content[0].text='Live preedit';
    const preeditResponse=await fetch(`${base}/api/preview-scene?width=320&scale=2&format=png`,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(previewDocument)});
    assert.equal(preeditResponse.status,200);
    const preeditBytes=await preeditResponse.arrayBuffer();
    const preeditLen=new DataView(preeditBytes).getUint32(0,true);
    const preeditScene=JSON.parse(new TextDecoder().decode(new Uint8Array(preeditBytes,4,preeditLen)));
    assert.deepEqual(preeditScene.document,previewDocument);
    assert.equal(preeditScene.format,"png");
    assert.ok(preeditBytes.byteLength < 100000);
    assert.deepEqual((await (await fetch(`${base}/api/state`)).json()).document,state.document);
    const stale=structuredClone(state.document);
    await action('insert_text',{text:'!',selection:caret(7)});
    const conflict=await fetch(`${base}/api/action`,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({command:'insert_text',text:'BAD',selection:caret(0),expected_document:stale})});
    assert.equal(conflict.status,409);
    const latest=await (await fetch(`${base}/api/state`)).json();
    assert.equal(latest.document.blocks[0].content.map(run=>run.text).join(''),'Selamat!');
  } finally {
    await fetch(`${base}/api/load`,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(original.document)});
  }
});
