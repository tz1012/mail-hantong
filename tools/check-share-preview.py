"""Check the two share documents, versioned image and synthetic-render provenance."""
from html.parser import HTMLParser
from pathlib import Path
import hashlib, json, struct

ROOT = Path(__file__).resolve().parents[1]
BASE = 'https://tz1012.github.io/mail-hantong/'
IMAGE = BASE+'assets/hantong-share-v1.png'
class Page(HTMLParser):
    def __init__(self):
        super().__init__(); self.meta={}; self.links=[]; self.images=[]
    def handle_starttag(self,tag,attrs):
        attrs=dict(attrs)
        if tag=='meta':
            key=attrs.get('property',attrs.get('name'))
            if key:
                assert key not in self.meta, f'Duplicate metadata: {key}'
                self.meta[key]=attrs.get('content')
        if tag=='a': self.links.append(attrs.get('href',''))
        if tag=='img': self.images.append(attrs)

for name,url in [('index.html',BASE),('download.html',BASE+'download.html')]:
    page=Page(); page.feed((ROOT/name).read_text(encoding='utf8'))
    assert page.meta['og:type']=='website'
    assert page.meta['og:url']==url
    assert page.meta['og:image']==page.meta['twitter:image']==IMAGE
    assert page.meta['og:image:width']=='1280' and page.meta['og:image:height']=='640'
    assert page.meta['og:image:alt'] and page.meta['og:title'] and page.meta['og:description']
    assert page.meta['twitter:card']=='summary_large_image'
    assert any(img.get('src')=='assets/hantong-tui-demo-v1.png' and img.get('alt') for img in page.images)
    for link in page.links:
        if link and not link.startswith(('https:','mailto:','#')):
            assert (ROOT/link.split('#')[0]).is_file(),link
    if name=='download.html':
        assert len([link for link in page.links if '/releases/download/' in link and link.endswith(('.zip','.tar.gz'))])==4

image=(ROOT/'assets/hantong-share-v1.png').read_bytes()
assert image[:8]==b'\x89PNG\r\n\x1a\n'
assert struct.unpack('>II',image[16:24])==(1280,640)
assert len(image)<1_000_000
proof=json.loads((ROOT/'assets/hantong-preview-provenance.json').read_text(encoding='utf8'))
assert proof['synthetic_data_only'] is True
for name,digest in proof['images'].items():
    assert hashlib.sha256((ROOT/'assets'/name).read_bytes()).hexdigest()==digest
print('2 share pages, 4 platform downloads, PNG dimensions/size, image hashes and synthetic provenance passed.')
