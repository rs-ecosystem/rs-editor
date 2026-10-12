import test from 'node:test';
import assert from 'node:assert/strict';
import {replacementDiff, compositionEdits, blockTexts, typingMarks, typingStyle, sceneMatches, wordRange, touchMovement, isDoubleTap, selectionOwner, preeditDocument, documentFingerprint} from '../web/input.mjs';
const apply = (before, edit) => {
  const parts = [...new Intl.Segmenter(undefined,{granularity:'grapheme'}).segment(before)].map(x => x.segment);
  return parts.slice(0,edit.selection.anchor.offset).join('') + edit.text + parts.slice(edit.selection.focus.offset).join('');
};
test('selection-only reconversion is a no-op, including emoji and RTL', () => {
  for (const text of ['', 'Selamat', '👩‍💻🙂', 'العربية', 'e\u0301']) assert.equal(replacementDiff(text, text), null);
});
test('native reconversion computes replacement rather than appending full word', () => {
  const edit = replacementDiff('Selamat', 'Selamat datang');
  assert.equal(edit.text, ' datang');
  assert.equal(edit.selection.anchor.offset, 7);
  assert.equal(apply('Selamat', edit), 'Selamat datang');
});
test('native deletion, correction, repeated letters and ZWJ edits preserve graphemes', () => {
  for (const [before, after] of [['a🙂b','ab'],['cat','dog'],['aaa','aaaa'],['👩‍💻🙂','👨‍💻🙂'],['e\u0301','e'],['','Selamat']]) {
    const edit = replacementDiff(before, after, 2);
    assert.equal(edit.selection.anchor.block_index, 2);
    assert.equal(apply(before, edit), after);
  }
});
test('bold override and explicit bold off are distinct from inherited marks', () => {
  const state = {document:{blocks:[{content:[{text:'abc',marks:[{type:'bold'}]}]}]},selection:{anchor:{block_index:0,offset:3},focus:{block_index:0,offset:3}}};
  assert.deepEqual(typingMarks(state), [{type:'bold'}]);
  assert.deepEqual(typingMarks({...state,active_marks:[]}), []);
  assert.deepEqual(typingMarks({...state,active_marks:[{type:'italic'}]}), [{type:'italic'}]);
});
test('live IME style retains heading and bold italic underline code formatting', () => {
  const style = typingStyle([{type:'bold'},{type:'italic'},{type:'underline'},{type:'code'}]);
  assert.equal(style.fontWeight, '700'); assert.equal(style.fontStyle, 'italic');
  assert.equal(style.textDecoration, 'underline'); assert.match(style.fontFamily,/OpenRich Mono/);
  assert.equal(typingStyle([],1).fontSize, '26.4px');
  assert.equal(typingStyle([],1).fontWeight, '700');
  assert.equal(typingStyle([]).fontWeight, '400');
});
test('stale scene cannot be used to hit test a newer document', () => {
  const scene = {document:{blocks:[{content:'old'}]}};
  assert.equal(sceneMatches(scene,JSON.stringify(scene.document)),true);
  assert.equal(sceneMatches(scene,JSON.stringify({blocks:[{content:'new'}]})),false);
  assert.equal(sceneMatches(null,'null'),false);
});

test('word selection works at final glyph and respects the visual hit affinity', () => {
  assert.deepEqual(wordRange('hello world',11),{start:6,end:11});
  assert.deepEqual(wordRange('hello world',5,'upstream'),{start:0,end:5});
  assert.deepEqual(wordRange('hello world',6,'downstream'),{start:6,end:11});
  assert.equal(wordRange('',0),null);
});
test('touch drag scrolls until hold activation, then extends selection', () => {
  const session={x:20,y:20,time:0,moved:false,selecting:false};
  assert.equal(touchMovement(session,22,22),'pending');
  assert.equal(touchMovement(session,20,40),'scroll');
  assert.equal(touchMovement({...session,selecting:true},20,40),'selection');
  assert.equal(touchMovement({...session,moved:true},20,20),'scroll');
});
test('double tap requires both time and distance thresholds', () => {
  const first={x:20,y:20,time:100};
  assert.equal(isDoubleTap(first,{x:22,y:21,time:300}),true);
  assert.equal(isDoubleTap(first,{x:60,y:21,time:300}),false);
  assert.equal(isDoubleTap(first,{x:22,y:21,time:500}),false);
  assert.equal(isDoubleTap(null,first),false);
});

test('replacement inherits first selected marks, caret inherits preceding marks', () => {
  const position = offset => ({block_index:0,offset});
  const state = {document:{blocks:[{content:[{text:'a',marks:[]},{text:'B',marks:[{type:'bold'}]},{text:'c',marks:[]}]}]}};
  for (const [anchor,focus] of [[1,2],[2,1]]) assert.deepEqual(typingMarks(state,{anchor:position(anchor),focus:position(focus)}),[{type:'bold'}]);
  assert.deepEqual(typingMarks(state,{anchor:position(1),focus:position(1)}),[]);
});
test('native composition and committed Skia selection have exclusive ownership', () => {
  assert.equal(selectionOwner(true),'skia');
  assert.equal(selectionOwner(false),'skia');
});

test('Skia preedit keeps mixed marks and original document immutable', () => {
  const document={schema_version:2,blocks:[{type:'paragraph',id:'1',content:[{type:'text',text:'a',marks:[]},{type:'text',text:'B🙂',marks:[{type:'bold'}]},{type:'text',text:'c',marks:[]}]}]};
  const copy=structuredClone(document);
  const preview=preeditDocument(document,['aB🙂Xc'],[{type:'bold'}]);
  assert.deepEqual(document,copy);
  assert.deepEqual(preview.blocks[0].content,[{type:'text',text:'a',marks:[]},{type:'text',text:'B🙂X',marks:[{type:'bold'}]},{type:'text',text:'c',marks:[]}]);
  assert.deepEqual(preeditDocument(document,['aZc'],[{type:'italic'}]).blocks[0].content,[{type:'text',text:'a',marks:[]},{type:'text',text:'Z',marks:[{type:'italic'}]},{type:'text',text:'c',marks:[]}]);
});

test('preview identity ignores JS versus Rust object field order', () => {
  assert.equal(documentFingerprint({content:[{type:'text',text:'B',marks:[{type:'bold'}]}]}),documentFingerprint({content:[{marks:[{type:'bold'}],text:'B',type:'text'}]}));
});

test('IME edits ignore structural blocks whose DOM text belongs to child nodes', () => {
  const document={schema_version:2,blocks:[{type:'paragraph',id:'1',content:[{type:'text',text:'Halo ',marks:[]}]},{type:'bullet_list',id:'2',items:[]},{type:'horizontal_rule',id:'3'}]};
  const snapshot=blockTexts(document);
  assert.deepEqual(snapshot,['Halo ',null,null]);
  assert.deepEqual(compositionEdits(snapshot,['Halo 日本','Anak',''],null),{edits:[{text:'日本',selection:{anchor:{block_index:0,offset:5,affinity:'downstream'},focus:{block_index:0,offset:5,affinity:'downstream'}}}],unsupported:false});
});
test('IME over a cross-block selection becomes one replacement of the whole range', () => {
  const at=(block_index,offset)=>({block_index,offset,affinity:'downstream'});
  const selection={anchor:at(1,4),focus:at(0,5)};
  const snapshot=['Satu dua','Tiga empat','Lain'];
  assert.deepEqual(compositionEdits(snapshot,['Satu あ empat',null,'Lain'],selection),{edits:[{text:'あ',selection:{anchor:at(0,5),focus:at(1,4)}}],unsupported:false});
  assert.deepEqual(compositionEdits(snapshot,['Satu  empat',null,'Lain'],selection).edits[0].text,'','cancelled preedit still deletes the replaced range');
  assert.deepEqual(compositionEdits(snapshot,snapshot,selection),{edits:[],unsupported:false},'untouched DOM is a cancel');
  assert.equal(compositionEdits(snapshot,['Satu あ empat','Tiga empat','Lain'],selection).unsupported,true,'unmerged blocks cannot be mapped');
  assert.equal(compositionEdits(snapshot,['Satu あ empat',null,'Lainnya'],selection).unsupported,true,'edits outside the range are rejected');
  const preview=preeditDocument({schema_version:2,blocks:snapshot.map((text,index)=>({type:'paragraph',id:String(index+1),content:[{type:'text',text,marks:[]}]}))},['Satu あ empat',null,'Lain'],[],selection);
  assert.deepEqual(preview.blocks.map(block=>block.content.map(run=>run.text).join('')),['Satu あ empat','Lain']);
});
test('IME edits in two blocks or in a removed block are rejected instead of guessed', () => {
  assert.equal(compositionEdits(['a','b'],['ax','bx'],null).unsupported,true);
  assert.equal(compositionEdits(['a','b'],['a',null],null).unsupported,true);
});
test('composition behind an in-flight insertion is moved to the logical caret', () => {
  const at=(block_index,offset)=>({block_index,offset,affinity:'downstream'});
  const logical={anchor:at(0,6),focus:at(0,6)};
  assert.deepEqual(compositionEdits(['Halo'],['Halo日本'],logical,at(0,4)).edits,[{text:'日本',selection:{anchor:at(0,6),focus:at(0,6)}}]);
  assert.deepEqual(compositionEdits(['Halo'],['Hallo'],{anchor:at(0,3),focus:at(0,3)},at(0,2)).edits[0].text,'l','ambiguous diffs use the stale caret, not the longest prefix');
  assert.deepEqual(compositionEdits(['Halo'],['Halo'],logical,at(0,4)).edits,[],'cancel inserts nothing');
  const reconversion=compositionEdits(['Halo dunia'],['Halo bumi'],logical,at(0,10));
  assert.deepEqual(reconversion.edits[0].selection,{anchor:at(0,5),focus:at(0,10)},'replacements elsewhere keep their own range');
  const document={schema_version:2,blocks:[{type:'paragraph',id:'1',content:[{type:'text',text:'Halo',marks:[]}]}]};
  const latest={schema_version:2,blocks:[{type:'paragraph',id:'1',content:[{type:'text',text:'Halo d',marks:[]}]}]};
  assert.equal(preeditDocument(document,['Halo愛'],[],logical,at(0,4),latest).blocks[0].content[0].text,'Halo d愛','preview uses the engine document once it caught up');
  assert.equal(preeditDocument(document,['Halo愛'],[],logical,at(0,4),document).blocks[0].content[0].text,'Halo','no misplaced preview before it caught up');
});
