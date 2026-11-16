#!/usr/bin/env python3
"""Membuat notch-core/src/eyes_data.rs dari src/css/eye-themes.css (20 skin x 8 mood).

Jalankan dari akar repo bila palet mata di CSS berubah:  python3 scripts/gen_eyes_rs.py
Setelah itu: cargo test -p notch-core eyes
"""
import re, collections

css = open('src/css/eye-themes.css').read()
css = re.sub(r'/\*.*?\*/', '', css, flags=re.S)
skin = collections.defaultdict(dict); pal = {}
for sel, body in re.findall(r'([^{}]+)\{([^{}]*)\}', css):
    sel = sel.strip()
    d = dict((k.strip(), v.strip()) for k, v in re.findall(r'(--[\w-]+)\s*:\s*([^;]+);', body))
    m = re.fullmatch(r'\.eyes\[data-mood="(\w+)"\]\[data-eye-theme="(\w+)"\]', sel)
    if m: pal[f"{m[2]}|{m[1]}"] = d; continue
    m = re.fullmatch(r'\.eyes\[data-eye-theme="(\w+)"\]', sel)
    if m: skin[m[1]].update(d)

order = ['aurora','neon','pastel','pixel','holo','ember','ocean','sakura','cyber','monochrome',
         'kawaii','boba','galaxy','matcha','candy','synthwave','slime','cotton','pumpkin','frost']
moods = ['normal','happy','sleepy','surprised','angry','love','suspicious','sad']

def f(x): 
    x = x.strip().rstrip('px')
    return float(x)

def split_top(s):
    out, depth, cur = [], 0, ''
    for ch in s:
        if ch == '(':depth += 1
        if ch == ')':depth -= 1
        if ch == ',' and depth == 0: out.append(cur.strip()); cur = ''
        else: cur += ch
    if cur.strip(): out.append(cur.strip())
    return out

def color(s):
    s = s.strip()
    if s == 'transparent': return None
    m = re.fullmatch(r'rgba\(var\(--eye-skin-rgb[^)]*\)\s*,\s*([\d.]+)\)', s)
    if m: return f"C::Skin({float(m[1])})"
    m = re.fullmatch(r'rgba\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*([\d.]+)\s*\)', s)
    if m: return f"C::Rgb({m[1]}, {m[2]}, {m[3]}, {float(m[4])})"
    raise SystemExit('color? ' + s)

def shadows(s):
    res = []
    for part in split_top(s):
        if part.endswith('transparent'): continue
        inset = part.startswith('inset')
        if inset: part = part[5:].strip()
        m = re.match(r'(-?[\d.]+)(?:px)?\s+(-?[\d.]+)(?:px)?\s+(-?[\d.]+)(?:px)?\s+(.*)$', part)
        dx, dy, bl, col = m[1], m[2], m[3], m[4]
        c = color(col)
        if c is None: continue
        res.append(f"Sh {{ inset: {str(inset).lower()}, dx: {float(dx)}, dy: {float(dy)}, blur: {float(bl)}, color: {c} }}")
    return res

def outline(s):
    m = re.match(r'([\d.]+)px solid (.*)$', s)
    if not m: return 'None'
    return f"Some(Outline {{ w: {float(m[1])}, color: {color(m[2])} }})"

out = ["// BERKAS HASIL GENERATOR: jangan edit manual.",
       "// Sumber: src/css/eye-themes.css (20 skin x 8 mood). Dibuat oleh tools/gen_eyes_rs.py.", "",
       "use crate::eyes::{Outline, PalEntry, Sh, SkinDef, C};", "",
       "pub static SKINS: [SkinDef; 20] = ["]
for name in order:
    p = skin[name]
    second = [int(x) for x in p.get('--eye-second-rgb', '0,0,0').split(',')]
    out.append("    SkinDef {")
    out.append(f'        name: "{name}",')
    out.append(f"        glow_extra: {f(p['--eye-glow-extra'])}, glow_alpha: {float(p['--eye-glow-alpha'])}, glow_soft: {float(p['--eye-glow-soft'])}, highlight: {float(p['--eye-highlight-alpha'])},")
    sh = shadows(p['--eye-offset-shadow'])
    out.append(f"        shadows: &[{', '.join(sh)}],")
    out.append(f"        outline: {outline(p['--eye-outline'])},")
    out.append(f"        second: [{second[0]}, {second[1]}, {second[2]}],")
    out.append("        pal: [")
    for m in moods:
        e = pal[f"{name}|{m}"]
        rgb = [int(x) for x in e['--eye-skin-rgb'].split(',')]
        ga = f"Some({float(e['--eye-glow-alpha'])})" if '--eye-glow-alpha' in e else 'None'
        out.append(f"            PalEntry {{ rgb: [{rgb[0]}, {rgb[1]}, {rgb[2]}], glow_alpha: {ga} }},")
    out.append("        ],")
    out.append("    },")
out.append("];")
open('notch-core/src/eyes_data.rs','w').write("\n".join(out)+"\n")
print('ok', len(out))
