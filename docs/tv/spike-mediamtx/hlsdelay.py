import re, sys, time, statistics, urllib.request, http.cookiejar
from datetime import datetime
name, secs = sys.argv[1], int(sys.argv[2])
cj = http.cookiejar.CookieJar(); op = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(cj))
base = f"http://127.0.0.1:8888/{name}/"
def get(u): return op.open(u, timeout=5).read().decode()
master = get(base + "index.m3u8")
vid = [l for l in master.splitlines() if l and not l.startswith('#') and 'video' in l][0]
parts_seen = 0; delays = []; ptargets = set(); segdur = None
t_end = time.time() + secs
while time.time() < t_end:
    t0 = time.time(); pl = get(base + vid); now = time.time()
    lines = pl.splitlines()
    m = re.search(r'#EXT-X-PART-INF:PART-TARGET=([\d.]+)', pl)
    if m: ptargets.add(float(m.group(1)))
    m = re.search(r'#EXT-X-TARGETDURATION:(\d+)', pl)
    if m: segdur = int(m.group(1))
    # percorre: PDT marca o início do segmento seguinte; soma as partes até ao fim
    pdt = None; acc = 0.0; last_end = None
    for l in lines:
        if l.startswith('#EXT-X-PROGRAM-DATE-TIME:'):
            pdt = datetime.fromisoformat(l.split(':',1)[1].replace('Z','+00:00')).timestamp(); acc = 0.0
        elif l.startswith('#EXT-X-PART:') and pdt is not None:
            d = float(re.search(r'DURATION=([\d.]+)', l).group(1)); acc += d; last_end = pdt + acc
            parts_seen += 1
        elif l.startswith('#EXTINF:') and pdt is not None:
            pass
    if last_end: delays.append(now - last_end)
    time.sleep(0.5)
delays = [d for d in delays]
delays.sort()
def pct(p): return delays[min(len(delays)-1, int(p/100*len(delays)))]
print(f"amostras={len(delays)} part_target={sorted(ptargets)} target_duration={segdur}s")
if delays:
    print(f"atraso da parte mais recente (s): min={delays[0]:.2f} p50={pct(50):.2f} p95={pct(95):.2f} max={delays[-1]:.2f}")
