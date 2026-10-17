// 画面・応答・DBの観測を別の欄に出す。表示だけから保存成功とは示さない。
const esc=value=>String(value??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
export function renderBrowserEvidence(evidence){
 const phase=evidence.execution?.containers,report=phase?.browser;if(!report)return '';
 return `<details class="browser-evidence"><summary>ブラウザの観測と画面写真</summary><p>ブラウザ ${esc(report.version||'版を未確認')}／${report.prepared?'準備完了':'準備未完了'}／${report.completed?'宣言した操作を完了':'操作未完了'}</p><p>${esc(report.detail)}</p><p>画面の表示、HTTP応答、DBの行をそれぞれ確認してください。</p>
 <h4>操作と表示</h4><table><thead><tr><th>手順</th><th>結果と表示の文字列</th><th>画面写真</th></tr></thead><tbody>${report.steps.map(s=>`<tr><td>${esc(s.id)}<br>${s.phase==='preparation'?'準備':'試験操作'}</td><td>${s.completed?'操作完了':'未完了'}<br>${esc(s.detail)}${s.text!=null?`<pre>${esc(s.text)}</pre>`:''}</td><td>${s.screenshot?`<button class="secondary browser-screenshot" data-evidence="${esc(evidence.id)}" data-step="${esc(s.id)}">${esc(s.id)}の画面写真を開く</button>`:'写真なし'}</td></tr>`).join('')}</tbody></table>
 <h4>ブラウザが受け取ったHTTP応答</h4>${report.responses.length?`<table><thead><tr><th>観測と要求時の手順</th><th>応答</th></tr></thead><tbody>${report.responses.map(r=>`<tr><td>${esc(r.watch)}／${esc(r.request_step??'対応を未確認')}<br>${esc(r.method)} ${esc(r.path)}</td><td>HTTP ${esc(r.status)}<br>${r.body_match==null?'本文は未観測':r.body_match?'指定文字列を含む':'指定文字列を含まない'}</td></tr>`).join('')}</tbody></table>`:'<p>対応する応答の記録なし</p>'}
 <h4>基盤がDBから直接読んだ行</h4>${(phase.database_reads||[]).map(q=>`<p><strong>${esc(q.id)}</strong>／${q.performed&&q.exit_code===0?'読取完了':'読取を確認できない'}<br>${esc(q.detail)}</p>${q.performed&&q.exit_code===0?`<pre>${esc(q.text||'一致する行なし')}</pre>`:''}`).join('')||'<p>直接読取の記録なし</p>'}
 <p>画面写真は撮影時の表示です。試行フォルダーを移動・削除した場合や、記録のJSONだけを復元した場合は開けません。判定と写真の有無は別に扱います。</p></details>`;
}
export function installBrowserScreenshots(token){
 document.addEventListener('click',async event=>{
  const button=event.target.closest?.('button.browser-screenshot');if(!button)return;
  const dialog=document.createElement('dialog');dialog.className='browser-photo-dialog';
  const title=document.createElement('h3');title.textContent=button.dataset.step+'の画面写真';
  const status=document.createElement('p');status.textContent='画面写真を読み込んでいます。';status.setAttribute('role','status');
  const close=document.createElement('button');close.textContent='画面写真を閉じる';close.addEventListener('click',()=>dialog.close());dialog.append(title,status,close);document.body.append(dialog);dialog.showModal();
  const abort=new AbortController();let objectURL;
  dialog.addEventListener('close',()=>{abort.abort();if(objectURL)URL.revokeObjectURL(objectURL);dialog.remove();},{once:true});
  try{
   const response=await fetch(`/api/v1/browser-screenshot/${encodeURIComponent(button.dataset.evidence)}/${encodeURIComponent(button.dataset.step)}`,{headers:{'X-Review-Token':token},signal:abort.signal});
   if(!response.ok){const body=await response.json();throw Error(body.error||'画面写真を読み出せません。');}
   if(response.headers.get('Content-Type')!=='image/png')throw Error('画面写真の形式が一致しません。');
   const blob=await response.blob();if(blob.size>4*1024*1024)throw Error('画面写真が上限を超えています。');
   if(!dialog.isConnected)return;objectURL=URL.createObjectURL(blob);const img=document.createElement('img');img.src=objectURL;img.alt=button.dataset.step+'を操作したときの画面';dialog.insertBefore(img,close);status.textContent='撮影時の表示です。保存内容はDBの観測欄で確認してください。';
  }catch(error){if(dialog.isConnected)status.textContent=error.message;}
 });
}
