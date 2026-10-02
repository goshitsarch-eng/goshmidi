(async()=>{
 const results=[];
 const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
 const q=selector=>document.querySelector(selector);
 const assert=(condition,message)=>{if(!condition)throw Error(message);};
 const wait=async(predicate,message)=>{for(let i=0;i<80;i++){if(predicate())return;await delay(50);}throw Error(message);};
 const click=async(selector)=>{await wait(()=>q(selector),'Missing '+selector);const node=q(selector);assert(node,'Missing '+selector);assert(!node.disabled,'Disabled '+selector);node.click();await delay(300);};
 const set=async(selector,value)=>{await wait(()=>q(selector),'Missing '+selector);const node=q(selector);assert(node,'Missing '+selector);const prototype=node.tagName==='SELECT'?HTMLSelectElement.prototype:HTMLInputElement.prototype;Object.getOwnPropertyDescriptor(prototype,'value').set.call(node,value);node.dispatchEvent(new Event(node.tagName==='SELECT'?'change':'input',{bubbles:true}));await delay(250);};
 const action=async id=>{q('#application-menu').open=true;await click('[data-action="'+id+'"]');};
 const stage=async name=>{results.push({name,status:'passed'});dioxus.send({stage:name,results,complete:false});await delay(1500);};
 try {
  await wait(()=>q('#play')&&!q('#play').disabled,'Song did not load');await click('#stop');
  await click('#play');await wait(()=>document.querySelector('footer').textContent.includes('Playing'),'Play failed');
  await click('#play');await wait(()=>document.querySelector('footer').textContent.includes('Paused'),'Pause failed');
  await click('#play');await set('#tempo','125');await set('#volume','80');await set('#pitch','2');await set('#seek','500');await click('#stop');
  await wait(()=>q('#seek').value==='0','Stop did not reset position');await click('#rewind');await click('#forward');
  await click('#jump');await set('#jump-bar','1');await click('#apply-jump');
  await click('#loop');await set('#loop-start','1');await set('#loop-end','2');await click('#apply-loop');assert(q('#loop').textContent==='Loop on','Loop did not apply');await click('#loop');
  for(const repeat of ['1','2','0'])await set('#repeat',repeat);await stage('transport');
  await click('#view-channels');assert(q('[data-channel]'),'No channel rows');
  const row=q('[data-channel]');const text=row.querySelector('input[type=text]');await set('[data-channel] input[type=text]','QA channel');
  for(const checkbox of [...row.querySelectorAll('input[type=checkbox]')]){checkbox.click();await delay(100);assert(checkbox.checked,'Mixer toggle did not apply');}
  await set('[data-channel] select','10');await set('[data-channel] input[type=range]','70');await stage('channels');
  await click('#view-piano');await click('#tighten');await click('#hide-unused');
  const key=q('[data-piano-note="60"]');assert(key,'Piano key absent');key.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true}));await delay(150);key.dispatchEvent(new PointerEvent('pointerup',{bubbles:true}));await click('#show-all');await stage('piano');
  await click('#view-lyrics');await set('#lyric-track','0');await set('#lyric-type','0');await set('#lyric-codec','UTF-8');await click('#autoscroll');
  const lyricText=q('#lyrics-body').textContent;await click('#copy-lyrics');await wait(()=>document.querySelector('footer').textContent.includes('Lyrics copied'),'Copy failed');
  await click('#play');await delay(450);await click('#stop');assert(q('#lyrics-body').textContent===lyricText,'Lyrics duplicated after playback');await stage('lyrics');
  await action('preferences');
  const fields=[...q('[role=dialog]').querySelectorAll('.form-row input,.form-row select')];assert(fields.length===18,'Expected all 18 preferences');
  for(const node of fields){if(node.type==='checkbox'){node.click();await delay(80);}else if(node.tagName==='SELECT'){node.value=String((Number(node.value)+1)%node.options.length);node.dispatchEvent(new Event('change',{bubbles:true}));await delay(80);}else{const label=node.closest('label').textContent;let value=node.type==='number'?(label.includes('Drums')?'10':'70'):label.toLowerCase().includes('color')?'#5277aa':'Sans Serif 14';Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(node,value);node.dispatchEvent(new Event('input',{bubbles:true}));await delay(80);}}
  await stage('preferences');await click('#apply-preferences');assert(!q('[role=dialog]'),'Preferences did not close');
  await action('preferences');await click('#cancel-dialog');
  await click('#midi-setup');await set('#midi-backend','Dummy');await wait(()=>q('#midi-port option'),'Ports did not enumerate');await click('#refresh-ports');await click('#midi-advanced');await set('#midi-map','1');await set('#midi-reset','1');await set('#midi-soundfont','');await click('#test-note');await delay(500);await stage('midi');await click('#apply-midi');
  for(const id of ['file-info','help','about']){await action(id);assert(q('[role=dialog]'),'Dialog did not open: '+id);await click('#done-dialog');}
  await action('save-settings');await wait(()=>document.querySelector('footer').textContent.includes('Song settings saved'),'Song settings save failed');await action('load-settings');
  await set('#theme','dark');await stage('dark');await set('#theme','light');await stage('light');await set('#theme','system');
  await click('#sidebar');assert(!q('aside'),'Playlist did not hide');await click('#sidebar');assert(q('aside'),'Playlist did not show');
  await click('#clear');assert(q('#discard'),'Unsaved changes were not protected');await click('#cancel-dialog');assert(q('[data-song]'),'Cancel lost playlist');
  await stage('dialogs-and-persistence');
  dioxus.send({complete:true,results,status:'passed',nativeDialogs:'outside the automated DOM suite',platform:navigator.platform});
 }catch(error){dioxus.send({complete:true,results,status:'failed',error:String(error),stack:error.stack});}
})();
