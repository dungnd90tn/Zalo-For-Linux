#!/usr/bin/env python3
"""Generate native catalogs from checked-in original CSS facts and TrueType cmaps."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import struct

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / 'rust-client/src/theme/catalog_data.json'


def cmap_glyph(data, codepoint):
    """Read Unicode format 4/12 mappings; reject unsupported or corrupt font tables."""
    u16 = lambda offset: struct.unpack_from('>H', data, offset)[0]
    u32 = lambda offset: struct.unpack_from('>I', data, offset)[0]
    count = u16(4)
    tables = {data[12 + i * 16:16 + i * 16]: (u32(20 + i * 16), u32(24 + i * 16)) for i in range(count)}
    start, length = tables[b'cmap']
    if start + length > len(data):
        raise ValueError('Truncated cmap')
    found = set()
    for i in range(u16(start + 2)):
        record = start + 4 + i * 8
        platform, encoding, offset = u16(record), u16(record + 2), start + u32(record + 4)
        if platform != 0 and not (platform == 3 and encoding in (1, 10)):
            continue
        fmt = u16(offset)
        if fmt == 12:
            for group in range(u32(offset + 12)):
                at = offset + 16 + group * 12
                first, last, glyph = u32(at), u32(at + 4), u32(at + 8)
                if first <= codepoint <= last:
                    found.add(glyph + codepoint - first)
        elif fmt == 4 and codepoint <= 0xffff:
            segments = u16(offset + 6) // 2
            ends = offset + 14
            starts = ends + 2 * segments + 2
            deltas, ranges = starts + 2 * segments, starts + 4 * segments
            for segment in range(segments):
                if u16(starts + segment * 2) <= codepoint <= u16(ends + segment * 2):
                    delta, span = u16(deltas + segment * 2), u16(ranges + segment * 2)
                    glyph = codepoint
                    if span:
                        glyph = u16(ranges + segment * 2 + span + 2 * (codepoint - u16(starts + segment * 2)))
                    found.add((glyph + delta) & 0xffff if glyph else 0)
                    break
    found.discard(0)
    if len(found) > 1:
        raise ValueError('Conflicting Unicode cmaps')
    return next(iter(found), 0)


def build():
    inventory = json.loads((ROOT / 'rust-client/UI_ASSET_INVENTORY.json').read_text())
    tokens, light, dark = {}, {}, {}
    for entry in inventory['custom_properties']:
        name = entry['name']
        fact = {k: entry[k] for k in ('source', 'selectors', 'at_rules', 'value')}
        if fact not in tokens.setdefault(name, []):
            tokens[name].append(fact)
        if not entry['at_rules']:
            if entry['selectors'] == [':root .dark']:
                if name in dark and dark[name] != entry['value']:
                    raise ValueError('Conflicting dark root declaration: ' + name)
                dark[name] = entry['value']
            if ':root' in entry['selectors']:
                if name in light and light[name] != entry['value']:
                    raise ValueError('Conflicting light root declaration: ' + name)
                light[name] = entry['value']
    fonts = {}
    for family, prefix in [('Zalo', 'zalo-font.'), ('Screencap', 'zalo-pc-screencap.')]:
        path = next(p for p in sorted((ROOT / 'pc-dist/fonts').glob('*.ttf')) if p.name.startswith(prefix))
        fonts[family] = {'path': path.relative_to(ROOT).as_posix(), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}
    if not any('.fa' in row['selectors'] and row['value'].endswith('zalo-font')
               for row in inventory['font_shorthand_rules']):
        raise ValueError('Missing original .fa font shorthand association')
    if not any(row['value'].replace('!important','').strip() == 'zalo-pc-screencap'
               and '[data-icon]:before' in row['selectors'] for row in inventory['font_family_rules']):
        raise ValueError('Missing original screencap font association')
    for family, css_family in [('Zalo','zalo-font'),('Screencap','zalo-pc-screencap')]:
        filename=Path(fonts[family]['path']).name
        if not any(any(d['name']=='font-family' and d['value']==css_family for d in face['declarations'])
                   and any(d['name']=='src' and filename in d['value'] for d in face['declarations'])
                   for face in inventory['font_faces']):
            raise ValueError('Font family is not bound to the original font asset')
    font_bytes = {family: (ROOT / facts['path']).read_bytes() for family, facts in fonts.items()}
    glyphs, rejected, conflicts = {}, [], []
    for entry in inventory['content_rules']:
        if len(entry['codepoints']) != 1:
            continue
        codepoint = int(entry['codepoints'][0][2:], 16)
        for selector in entry['selectors']:
            match = re.search(r'\.([\w-]+)::?(?:before|after)$', selector)
            if not match:
                continue
            name = match[1]
            family = 'Zalo' if name.startswith('fa-') else 'Screencap' if name.startswith('icon-') else None
            if family is None:
                continue
            glyph_index = cmap_glyph(font_bytes[family], codepoint)
            facts = {'class': name, 'family': family, 'codepoint': codepoint, 'glyph_index': glyph_index}
            if not glyph_index:
                if facts not in rejected:
                    rejected.append(facts)
                continue
            previous = glyphs.get(name)
            if previous and previous != facts:
                conflicts.append([previous, facts])
            else:
                glyphs[name] = facts
    if conflicts:
        raise ValueError('Conflicting icon alias bindings: ' + str(conflicts[:3]))
    # Complete provenance remains in UI_ASSET_INVENTORY.json. The executable
    # needs every unique raw value, not repeated selector/source strings.
    runtime_tokens = {name: sorted({row['value'] for row in rows}) for name, rows in tokens.items()}
    return {'schema_version': 1, 'sources': inventory['css_sources'], 'tokens': runtime_tokens,
            'light': light, 'dark': dark, 'fonts': fonts,
            'glyphs': sorted(glyphs.values(), key=lambda row: row['class']),
            'missing_font_glyphs': rejected}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    data = build()
    encoded = json.dumps(data, ensure_ascii=True, sort_keys=True, separators=(',', ':')) + '\n'
    if args.check:
        if not OUTPUT.is_file() or OUTPUT.read_text() != encoded:
            raise SystemExit('Native UI catalog is stale; run scripts/rust/generate-ui-catalog.py')
    else:
        OUTPUT.write_text(encoded)
    print(json.dumps({key: len(data[key]) for key in ('tokens', 'light', 'dark', 'glyphs', 'missing_font_glyphs')}))


if __name__ == '__main__':
    main()
