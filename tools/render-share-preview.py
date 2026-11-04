"""Render a real beta.22 TUI frame with synthetic mail, then compose share cards.

Usage: python tools/render-share-preview.py /path/to/beta22-source
Requires Rust and Pillow only for authoring; neither is a website dependency.
"""
from pathlib import Path
import hashlib, json, re, shutil, subprocess, sys, tempfile, unicodedata
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
SOURCE = Path(sys.argv[1]).resolve()
OUT = ROOT / 'assets'
OUT.mkdir(exist_ok=True)
RENDERER = SOURCE / 'apps/hantong-t/src/terminal_view.rs'
with tempfile.TemporaryDirectory(prefix='hantong-share-render-') as temporary:
    stage = Path(temporary)
    shutil.copyfile(ROOT/'tools/share-preview.rs', stage/'main.rs')
    shutil.copyfile(RENDERER, stage/'terminal_view.rs')
    (stage/'resources').mkdir()
    # include_str! is relative to terminal_view.rs's source directory.
    for lang in ('en','ko'):
        shutil.copyfile(SOURCE/f'apps/hantong-t/resources/tui-{lang}.txt', stage/'resources'/f'tui-{lang}.txt')
    (stage/'src').mkdir()
    shutil.move(stage/'main.rs',stage/'src/main.rs')
    shutil.move(stage/'terminal_view.rs',stage/'src/terminal_view.rs')
    executable=stage/('preview.exe' if sys.platform=='win32' else 'preview')
    subprocess.run(['rustc','--edition=2024','-A','dead_code',str(stage/'src/main.rs'),'-o',str(executable)],check=True)
    ansi=subprocess.check_output([str(executable)]).decode('utf8')
    card_ansi=subprocess.check_output([str(executable),'144','26']).decode('utf8')

def color(n):
    if n<16:
        return ['#000000','#800000','#008000','#808000','#000080','#800080','#008080','#c0c0c0',
                '#808080','#ff0000','#00ff00','#ffff00','#0000ff','#ff00ff','#00ffff','#ffffff'][n]
    if n>=232: return (8+(n-232)*10,)*3
    n-=16; values=[0,95,135,175,215,255]
    return values[n//36],values[n//6%6],values[n%6]

def parse_cells(ansi,columns,rows):
    cells=[[(' ', '#afd7ff', '#000087') for _ in range(columns)] for _ in range(rows)]
    x=y=0; fg='#afd7ff'; bg='#000087'; i=0
    while i<len(ansi):
        if ansi[i]=='\x1b':
            match=re.match(r'\x1b\[([0-9;?]*)([A-Za-z])',ansi[i:]); assert match
            params,command=match.groups()
            values=[int(v) for v in params.split(';') if v and not v.startswith('?')]
            if command=='H': y=(values[0] if values else 1)-1; x=(values[1] if len(values)>1 else 1)-1
            if command=='m':
                if not values or values==[0]: fg='#dcdcdc'; bg='#000000'
                elif values[:2]==[38,5]: fg=color(values[2])
                elif values[:2]==[48,5]: bg=color(values[2])
                else: raise AssertionError('Unsupported renderer color')
            i+=len(match.group()); continue
        char=ansi[i]
        if char=='\n': x=0; y+=1; i+=1; continue
        width=2 if unicodedata.east_asian_width(char) in 'WF' else 1
        assert 0<=y<rows and 0<=x<columns
        cells[y][x]=(char,fg,bg)
        if width==2 and x+1<columns: cells[y][x+1]=('',fg,bg)
        x+=width; i+=1
    return cells

fontroot=Path('C:/Windows/Fonts')
latin=ImageFont.truetype(str(fontroot/'consola.ttf'),20)
cjk=ImageFont.truetype(str(fontroot/'malgun.ttf'),20)
bold=lambda size:ImageFont.truetype(str(fontroot/'malgunbd.ttf'),size)
regular=lambda size:ImageFont.truetype(str(fontroot/'malgun.ttf'),size)
def rasterize(ansi,columns,rows):
    frame=Image.new('RGB',(columns*12,rows*26),'#000087'); draw=ImageDraw.Draw(frame)
    for row,line in enumerate(parse_cells(ansi,columns,rows)):
        for col,(_,_,background) in enumerate(line):
            draw.rectangle((col*12,row*26,col*12+11,row*26+25),fill=background)
        for col,(char,foreground,_) in enumerate(line):
            if char and char!=' ': draw.text((col*12,row*26),char,font=cjk if ord(char)>127 else latin,fill=foreground)
    return frame
frame=rasterize(ansi,128,32)

full=Image.new('RGB',(1600,992),'#101b28'); d=ImageDraw.Draw(full)
d.text((32,20),'한통 · 메일을 한곳에서',font=bold(34),fill='#f3f7ff')
d.text((34,70),'실제 TUI 렌더러 / 가상 계정·메일 / beta.22',font=regular(20),fill='#a9c5d9')
full.paste(frame,(32,118)); full.save(OUT/'hantong-tui-demo-v1.png',optimize=True)

card=Image.new('RGB',(1280,640),'#f7f7f2'); d=ImageDraw.Draw(card)
d.text((40,17),'한통',font=bold(52),fill='#185b47')
d.text((178,32),'메일과 AI를 한 터미널에서',font=bold(31),fill='#1e302a')
d.text((44,91),'Windows · macOS · Linux    |    CLI + 통신풍 TUI',font=regular(20),fill='#52635a')
mini=rasterize(card_ansi,144,26)
mini.thumbnail((1196,468),Image.Resampling.LANCZOS)
card.paste(mini,(42,140))
d.text((896,616),'가상 데이터 · 실제 TUI 화면',font=regular(16),fill='#52635a')
card.save(OUT/'hantong-share-v1.png',optimize=True)
assert (OUT/'hantong-share-v1.png').stat().st_size<1_000_000
assert Image.open(OUT/'hantong-share-v1.png').size==(1280,640)
assert 'example.invalid' in ansi and '실제 계정이나 발송 없음' in ansi
(OUT/'hantong-preview-provenance.json').write_text(json.dumps({
    'product_renderer_sha256':hashlib.sha256(RENDERER.read_bytes()).hexdigest(),
    'source_release':'v0.1.0-beta.22','synthetic_data_only':True,
    'capture_method':'unmodified product terminal renderer with a synthetic Frame; ANSI cells rasterized',
    'images':{name:hashlib.sha256((OUT/name).read_bytes()).hexdigest() for name in ['hantong-share-v1.png','hantong-tui-demo-v1.png']},
},ensure_ascii=False,indent=2)+'\n',encoding='utf8')
print(json.dumps({'share_size':(OUT/'hantong-share-v1.png').stat().st_size,'dimensions':[1280,640],'synthetic_only':True}))
