# Makes many spoken MP3 files with one ElevenLabs voice. usage: ELEVEN_API_KEY=... make_batch.py OUT_DIR VOICE_ID LINES.tsv
# LINES.tsv: file_name<TAB>text . Existing files are kept, so a second run only makes the missing ones.
import sys, os, json, subprocess, urllib.request
out,voice,lines=sys.argv[1:4]; used=0
TRIM="silenceremove=start_periods=1:start_threshold=-45dB,areverse,silenceremove=start_periods=1:start_threshold=-45dB,areverse,loudnorm=I=-16:TP=-1.5"
for row in open(lines):
    if not row.strip(): continue
    name,text=row.rstrip("\n").split("\t"); dst=f"{out}/{name}.mp3"
    if os.path.exists(dst): continue
    body={"text":text,"model_id":"eleven_multilingual_v2","voice_settings":{"stability":0.5,"similarity_boost":0.8,"style":0.2}}
    req=urllib.request.Request(f"https://api.elevenlabs.io/v1/text-to-speech/{voice}?output_format=mp3_44100_128",
        data=json.dumps(body).encode(),headers={"xi-api-key":os.environ["ELEVEN_API_KEY"],"Content-Type":"application/json"})
    try: raw=urllib.request.urlopen(req,timeout=60).read()
    except urllib.error.HTTPError as e: sys.exit("HTTP %s %s"%(e.code,e.read()[:300]))
    open("/tmp/_raw.mp3","wb").write(raw); used+=len(text)
    subprocess.run(["ffmpeg","-loglevel","error","-y","-i","/tmp/_raw.mp3","-af",TRIM,"-ar","44100","-b:a","128k",dst],check=True)
    print(name,"|",text)
print("characters used:",used)
