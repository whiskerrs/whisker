from pathlib import Path
import json
root=Path('/tmp/giga-detailed-profile');data={}
for d in root.glob('perfetto-*'):
 if (d/'scopes.json').exists():data[d.name]={'metadata':json.loads((d/'metadata.json').read_text()),'scopes':json.loads((d/'scopes.json').read_text()),'ticks':json.loads((d/'worst-ticks.json').read_text())}
for d in root.glob('simpleperf-*'):
 if not (d/'cpu-summary.json').exists():continue
 meta=json.loads((d/'cpu-summary.json').read_text());samples=json.loads((d/'samples.json').read_text());tree={'name':'UI thread','value':0,'children':{}}
 for s in samples:
  if s['tid']!=meta['main_tid']:continue
  node=tree;node['value']+=s['period']
  for name in reversed(s['frames']):
   node=node['children'].setdefault(name,{'name':name,'value':0,'children':{}});node['value']+=s['period']
 data[d.name]={'cpu':meta,'tree':tree}
html='''<!doctype html><meta charset="utf-8"><title>GIGA Android — detailed CPU profile</title>
<style>body{font:15px system-ui;margin:32px;background:#f7f8fa;color:#18212e}h1{font-size:26px}p{max-width:1000px;line-height:1.7}select,input,button{font:inherit;padding:8px;margin:8px 8px 16px 0}table{border-collapse:collapse;background:white;margin-bottom:24px;width:100%}th,td{padding:8px 12px;border-bottom:1px solid #e5e7eb;text-align:right}th:first-child,td:first-child{text-align:left}code{font-size:12px;overflow-wrap:anywhere}#flame{background:white;overflow:auto}svg text{font-size:11px;pointer-events:none}svg g{cursor:pointer}.note{color:#596577}details{background:#fff;padding:10px;margin-bottom:8px}summary{cursor:pointer}</style>
<h1>GIGA Android：修正後debugビルドの詳細プロファイル</h1>
<p>Androidエミュレーター API 37。simpleperf と Perfetto は別実行です。計測中の処理時間には計測負荷が含まれ、通常実行のフレーム時間と直接比較できません。Perfettoの区間は入れ子なので、親と子の時間を足さないでください。</p>
<select id="sel"></select><span id="hint" class="note"></span><div id="body"></div>
<script>const DATA=__DATA__;
const sel=document.getElementById('sel'),body=document.getElementById('body');
for(const name of Object.keys(DATA).sort()){const o=document.createElement('option');o.value=o.textContent=name;sel.append(o)}
function el(tag,text){const e=document.createElement(tag);if(text!==undefined)e.textContent=text;return e}
function table(headers,rows){const t=el('table'),tr=el('tr');headers.forEach(x=>tr.append(el('th',x)));t.append(tr);for(const row of rows){const r=el('tr');row.forEach(x=>r.append(el('td',x)));t.append(r)}return t}
const fmt=n=>Number(n).toFixed(2);
function render(){body.replaceChildren();const d=DATA[sel.value];if(d.scopes){
 body.append(el('h2','処理区間の集計'));
 body.append(table(['区間','回数','合計 ms（内包）','平均 ms','p95 ms','最大 ms','CPU Running ms'],d.scopes.map(s=>[s.name,s.count,...['total_ms','avg_ms','p95_ms','max_ms','running_ms'].map(k=>fmt(s[k]))])));
 body.append(el('h2','時間の長いRust tick'));
 for(const t of d.ticks){const det=el('details');det.append(el('summary',`tick ${t.id}: ${fmt(t.wall_ms)} ms / CPU Running ${fmt(t.running_ms)} ms`));det.append(table(['区間','内包時間 ms'],Object.entries(t.scopes_ms).map(([k,v])=>[k,fmt(v)])));body.append(det)}
 }else{
 body.append(el('p',`${d.cpu.main_samples} UIスレッドサンプル。CPU時間推定 ${fmt(d.cpu.sampled_main_cpu_ms)} ms。幅はサンプリングされたCPU時間比率です。ボックスをクリックすると拡大します。`));
 const reset=el('button','全体に戻る'),search=el('input');search.placeholder='関数名で強調';body.append(reset,search);const wrap=el('div');wrap.id='flame';body.append(wrap);
 let current=d.tree;function draw(){wrap.replaceChildren();const svg=document.createElementNS('http://www.w3.org/2000/svg','svg');const width=Math.max(950,body.clientWidth);svg.setAttribute('width',width);let maxDepth=0;
 function rec(node,x,y,w){if(w<.8)return;maxDepth=Math.max(maxDepth,y);const g=document.createElementNS(svg.namespaceURI,'g'),rect=document.createElementNS(svg.namespaceURI,'rect');rect.setAttribute('x',x);rect.setAttribute('y',y*22);rect.setAttribute('width',Math.max(0,w-1));rect.setAttribute('height',21);let hash=0;for(const ch of node.name)hash=(hash*31+ch.charCodeAt(0))|0;rect.setAttribute('fill',search.value&&node.name.includes(search.value)?'#c763ed':`hsl(${20+Math.abs(hash)%35} 85% ${65+Math.abs(hash)%16}%)`);g.append(rect);const title=document.createElementNS(svg.namespaceURI,'title');title.textContent=`${node.name}\n${fmt(node.value/1e6)} sampled CPU ms (${fmt(node.value/d.tree.value*100)}%)`;g.append(title);if(w>40){const text=document.createElementNS(svg.namespaceURI,'text');text.setAttribute('x',x+3);text.setAttribute('y',y*22+15);text.textContent=node.name.slice(0,Math.floor(w/6.2));g.append(text)}g.onclick=()=>{current=node;draw()};svg.append(g);let next=x;for(const c of Object.values(node.children).sort((a,b)=>b.value-a.value)){const cw=w*c.value/node.value;rec(c,next,y+1,cw);next+=cw}}
 rec(current,0,0,width);svg.setAttribute('height',(maxDepth+1)*22);wrap.append(svg)}
 reset.onclick=()=>{current=d.tree;draw()};search.oninput=draw;draw();
 body.append(el('h2','CPUサンプリング：呼び出し先を含む時間'));
 body.append(table(['関数','UI CPU比率','推定CPU ms'],d.cpu.inclusive.slice(0,80).map(s=>[s.name,fmt(s.percent)+'%',fmt(s.cpu_ms)])));
 body.append(el('h2','CPUサンプリング：関数自身の時間'));
 body.append(table(['関数','UI CPU比率','推定CPU ms'],d.cpu.self.slice(0,50).map(s=>[s.name,fmt(s.percent)+'%',fmt(s.cpu_ms)])));
 }}sel.onchange=render;sel.value=location.hash.slice(1)||"perfetto-scroll";render();</script>'''
(root/'report.html').write_text(html.replace('__DATA__',json.dumps(data,ensure_ascii=False).replace('</','<\\/')))
print('Wrote',root/'report.html')
