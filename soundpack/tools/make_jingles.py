# Makes short jingles as WAV files. usage: make_jingles.py OUT_DIR
import sys, wave, numpy as np
SR=44100
N={'C':0,'D':2,'E':4,'F':5,'G':7,'A':9,'B':11}
def hz(n):
    name,octv=n[:-1],int(n[-1]); s=N[name[0]]+(1 if '#' in name else 0)-(1 if 'b' in name else 0)
    return 440*2**((s-9)/12+(octv-4))
def env(n,a,d):
    t=np.arange(n)/SR; return np.minimum(1,t/a)*np.exp(-t/d)
def bell(f,dur):
    n=int(dur*SR); t=np.arange(n)/SR
    return (np.sin(2*np.pi*f*t)+0.45*np.sin(2*np.pi*2.01*f*t)*np.exp(-t/0.25)+0.2*np.sin(2*np.pi*3.99*f*t)*np.exp(-t/0.12))*env(n,0.004,dur/3.2)
def chip(f,dur,duty=0.25):
    n=int(dur*SR); t=np.arange(n)/SR
    sq=np.where((t*f)%1<duty,1.0,-1.0); e=np.ones(n); r=int(0.02*SR); e[-r:]=np.linspace(1,0,r); e[:200]=np.linspace(0,1,200)
    return 0.45*sq*e*np.exp(-t/(dur*2.5))
def soft(f,dur):
    n=int(dur*SR); t=np.arange(n)/SR
    return (np.sin(2*np.pi*f*t)+0.3*np.sin(2*np.pi*2*f*t))*np.sin(np.pi*np.minimum(1,t/dur))**0.6*np.exp(-t/(dur*0.9))
def seq(voice,notes,tail=0.5):
    # notes: (start_s, name or [names], dur_s, gain)
    total=max(s+d for s,_,d,_ in notes)+tail; out=np.zeros(int(total*SR))
    for s,nm,d,g in notes:
        for one in (nm if isinstance(nm,list) else [nm]):
            w=voice(hz(one),d)*g; i=int(s*SR); out[i:i+len(w)]+=w
    return out
def echo(x,delay=0.11,g=0.28,times=3):
    out=np.concatenate([x,np.zeros(int(delay*times*SR))])
    for k in range(1,times+1):
        i=int(delay*k*SR); out[i:i+len(x)]+=x*g**k
    return out
def save(path,x):
    x=x/np.max(np.abs(x))*0.8; f=int(0.01*SR); x[-f:]*=np.linspace(1,0,f)
    with wave.open(path,'wb') as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(SR); w.writeframes((x*32767).astype('<i2').tobytes())
J={
 'j01_wake_bell':    echo(seq(bell,[(0,'C5',.5,1),(.13,'E5',.5,1),(.26,'G5',.5,1),(.39,'C6',.9,1)])),
 'j02_wake_chip':    seq(chip,[(0,'C5',.09,1),(.09,'E5',.09,1),(.18,'G5',.09,1),(.27,'C6',.09,1),(.36,'E6',.09,1),(.45,'G6',.28,1)],.15),
 'j03_done_bell':    echo(seq(bell,[(0,'G5',.35,1),(.16,'C6',.9,1),(.16,'E6',.9,.6)])),
 'j04_done_chip':    seq(chip,[(0,'E5',.08,1),(.08,'G5',.08,1),(.16,'E6',.08,1),(.24,'C6',.08,1),(.32,'D6',.08,1),(.40,'G6',.3,1)],.15),
 'j05_message_ping': echo(seq(bell,[(0,'E6',.25,1),(.12,'A6',.7,.9)]),.09,.3,2),
 'j06_error_low':    seq(chip,[(0,'E4',.14,1),(.17,'Eb4',.14,1),(.34,'C4',.4,1)],.15),
 'j07_error_bell':   echo(seq(bell,[(0,['E5','Bb5'],.3,.8),(.22,['Eb5','A5'],.8,.8)])),
 'j08_thinking':     seq(chip,[(i*.16,n,.07,.7) for i,n in enumerate(['C6','G5','C6','G5','E6','G5'])],.1),
 'j09_listening':    seq(soft,[(0,'G5',.18,1),(.14,'D6',.35,1)],.15),
 'j10_goodnight':    echo(seq(bell,[(0,'E6',.6,.9),(.28,'C6',.6,.9),(.56,'G5',.6,.9),(.84,['C5','E5'],1.4,.8)]),.16,.3,3),
 'j11_coin':         seq(chip,[(0,'B5',.07,1),(.07,'E6',.35,1)],.1),
 'j12_level_up':     seq(chip,[(0,'G5',.1,1),(.1,'C6',.1,1),(.2,'E6',.1,1),(.3,'G6',.1,1),(.4,'C6',.1,1),(.5,'E6',.1,1),(.6,'G6',.1,1),(.7,'C7',.4,1)],.2),
}
for k,v in J.items(): save(f"{sys.argv[1]}/{k}.wav",v); print(k, "%.2fs"%(len(v)/SR))
