#!/usr/bin/env python3
"""Local pages for the CEF spike. Binds to 127.0.0.1 only."""
import http.server, pathlib, sys

HERE = pathlib.Path(sys.argv[2]) if len(sys.argv) > 2 else pathlib.Path(".")
LOG = """<script>
const say=m=>console.log('FX '+m);
addEventListener('click',e=>say(`click ${e.clientX},${e.clientY}`));
addEventListener('wheel',e=>say(`wheel ${e.deltaY}`));
addEventListener('pointermove',e=>say(`move ${e.clientX},${e.clientY}`),{once:true});
addEventListener('load',()=>say('loaded top='+(self===top)+' size='+innerWidth+'x'+innerHeight));
</script>"""
PAGES = {
  "/deny": (LOG + "<body style='height:4000px;background:#264'><h1>refuses framing</h1>", True),
  "/parent": ("<script>addEventListener('load',()=>setTimeout(()=>{const f=document.querySelector('iframe');let r;try{r=f.contentDocument.body.innerText}catch(e){r='cross-origin:'+e.name}console.log('FX iframe body='+JSON.stringify(r))},1500))</script><iframe src='/deny' width=800 height=400></iframe>", False),
  "/audio": (LOG + """<canvas id=c width=1920 height=1080></canvas><script>
const x=c.getContext('2d');let n=0;(function d(){x.fillStyle=`hsl(${n++%360} 80% 50%)`;x.fillRect(0,0,1920,1080);requestAnimationFrame(d)})();
const a=new AudioContext();const o=a.createOscillator();o.connect(a.destination);o.start();a.resume().then(()=>say('audio '+a.state));
</script>""", False),
  "/video": ("""<body style='margin:0;background:#000'><video id=v autoplay loop width=1920 height=1080></video><canvas id=c width=1920 height=1080 hidden></canvas><script>
for (const t of ['video/mp4; codecs="avc1.42E01E"','video/mp4; codecs="hvc1.1.6.L93.B0"','video/webm; codecs="vp9"','video/webm; codecs="vp8"']) console.log('FX canPlay '+t+' => '+(document.createElement('video').canPlayType(t)||'no'));
const x=c.getContext('2d');let n=0;const draw=()=>{x.fillStyle=`hsl(${n%360} 80% 40%)`;x.fillRect(0,0,1920,1080);x.fillStyle='#fff';x.font='200px sans-serif';for(let i=0;i<9;i++)x.fillText(n+i,(n*7+i*210)%1920,120+i*115);n++};
const t=setInterval(draw,33);
const a=new AudioContext();const o=a.createOscillator();const d=a.createMediaStreamDestination();o.connect(d);o.start();
const s=c.captureStream(30);s.addTrack(d.stream.getAudioTracks()[0]);
const r=new MediaRecorder(s,{mimeType:'video/webm;codecs=vp8,opus',videoBitsPerSecond:8000000});const parts=[];r.ondataavailable=e=>parts.push(e.data);
r.onstop=()=>{clearInterval(t);v.src=URL.createObjectURL(new Blob(parts,{type:'video/webm'}));v.play().catch(e=>console.log('FX play '+e.name))};
r.start();setTimeout(()=>r.stop(),6000);
v.onplaying=()=>console.log('FX playing '+v.videoWidth+'x'+v.videoHeight+' muted='+v.muted);v.onerror=()=>console.log('FX video error '+v.error.code);
</script>""", False),
  "/signin": ("""<h1>sign in fixture</h1><script>
console.log('FX init='+window.__kairoInit+' cookie='+document.cookie);
document.cookie='kairo=1; max-age=86400';
navigator.mediaDevices.getUserMedia({audio:true}).then(()=>console.log('FX mic granted'),e=>console.log('FX mic '+e.name));
navigator.geolocation.getCurrentPosition(()=>console.log('FX geo granted'),e=>console.log('FX geo denied '+e.code));
setTimeout(()=>{const w=window.open('https://example.com/');console.log('FX popup='+!!w)},1000);
setTimeout(()=>{location.href='https://example.com/blocked'},2500);
setTimeout(()=>console.log('FX still '+location.href),5000);
</script>""", False),
}
class H(http.server.BaseHTTPRequestHandler):
    def log_message(self,*a): pass
    def do_GET(self):
        if self.path == "/clip.webm":
            p = HERE/"clip.webm"; data = p.read_bytes(); self.send_response(200); self.send_header("Content-Type","video/webm"); self.send_header("Content-Length",str(len(data))); self.end_headers(); self.wfile.write(data); return
        page = PAGES.get(self.path)
        if not page: self.send_response(404); self.end_headers(); return
        body, deny = page; data = body.encode()
        self.send_response(200); self.send_header("Content-Type","text/html; charset=utf-8"); self.send_header("Set-Cookie", "server=1; Max-Age=86400")
        if deny:
            self.send_header("X-Frame-Options","DENY"); self.send_header("Content-Security-Policy","frame-ancestors 'none'; default-src 'self' 'unsafe-inline'")
        self.send_header("Content-Length",str(len(data))); self.end_headers(); self.wfile.write(data)
http.server.ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
