#!/usr/bin/env python3
"""Inventory original CSS declarations/assets and native bindings, without evaluating CSS."""
import argparse
import hashlib
import json
from pathlib import Path
import re
from functools import lru_cache

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = Path('rust-client/UI_ASSET_INVENTORY.json')


@lru_cache(maxsize=None)
def digest(path):
    data = path.read_bytes()
    return {'path': path.relative_to(ROOT).as_posix(), 'bytes': len(data),
            'sha256': hashlib.sha256(data).hexdigest()}


def split_css(text, delimiter, maxsplit=None):
    """Split only outside strings, escapes, comments and parenthesized functions."""
    result, start, quote, depth, i = [], 0, None, 0, 0
    while i < len(text):
        ch = text[i]
        if ch == '\\':
            i += 2
            continue
        if quote:
            if ch == quote:
                quote = None
        elif ch in '\"\'':
            quote = ch
        elif ch in '([':
            depth += 1
        elif ch in ')]':
            depth -= 1
        elif not depth and ch == delimiter and (maxsplit is None or len(result) < maxsplit):
            result.append(text[start:i].strip())
            start = i + 1
        i += 1
    result.append(text[start:].strip())
    return result


def without_comments(text):
    out, quote, i = [], None, 0
    while i < len(text):
        ch = text[i]
        if ch == '\\':
            out.append(text[i:i + 2]); i += 2; continue
        if quote:
            if ch == quote:
                quote = None
        elif ch in '\"\'':
            quote = ch
        elif text.startswith('/*', i):
            end = text.find('*/', i + 2)
            if end < 0:
                raise ValueError('Unterminated CSS comment')
            out.append(' ' * (end + 2 - i)); i = end + 2; continue
        out.append(ch); i += 1
    if quote:
        raise ValueError('Unterminated CSS string')
    return ''.join(out)


def scan_css(text):
    """Return raw qualified rules plus enclosing at-rules; preserve declaration values."""
    text = without_comments(text)
    rules = []

    def walk(start, end, contexts):
        cursor, statement, quote, parens = start, start, None, 0
        while cursor < end:
            ch = text[cursor]
            if ch == '\\':
                cursor += 2; continue
            if quote:
                if ch == quote:
                    quote = None
            elif ch in '\"\'':
                quote = ch
            elif ch in '([':
                parens += 1
            elif ch in ')]':
                parens -= 1
            elif not parens and ch == ';':
                statement = cursor + 1
            elif not parens and ch == '{':
                header = text[statement:cursor].strip()
                depth, close, inner_quote = 1, cursor + 1, None
                while close < end and depth:
                    current = text[close]
                    if current == '\\':
                        close += 2; continue
                    if inner_quote:
                        if current == inner_quote:
                            inner_quote = None
                    elif current in '\"\'':
                        inner_quote = current
                    elif current == '{':
                        depth += 1
                    elif current == '}':
                        depth -= 1
                    close += 1
                if depth:
                    raise ValueError('Unbalanced CSS block')
                if header.startswith('@') and not header.lower().startswith('@font-face'):
                    walk(cursor + 1, close - 1, contexts + [header])
                else:
                    declarations = []
                    for declaration in split_css(text[cursor + 1:close - 1], ';'):
                        parts = split_css(declaration, ':', maxsplit=1)
                        if len(parts) >= 2:
                            declarations.append({'name': parts[0], 'value': parts[1]})
                    rules.append({'rule': len(rules), 'offset': statement,
                                  'selectors': split_css(header, ','), 'at_rules': contexts,
                                  'declarations': declarations})
                cursor = close; statement = close; continue
            elif not parens and ch == '}':
                raise ValueError('Unexpected CSS closing brace')
            cursor += 1
        if quote or parens:
            raise ValueError('Unbalanced CSS string/function')

    walk(0, len(text), [])
    return rules


def unescape_css(text):
    def replace(match):
        if match[1]:
            value = int(match[1], 16)
            return chr(value) if 0 < value <= 0x10ffff and not 0xd800 <= value <= 0xdfff else '\ufffd'
        return '' if match[2] in ('\n', '\r', '\f') else match[2]
    return re.sub(r'\\(?:([0-9a-fA-F]{1,6})[ \t\r\n\f]?|([\s\S]))', replace, text)


STRINGS = re.compile(r'''(["'])((?:\\[\s\S]|(?!\1)[^\\])*)\1''')
CLASSES = re.compile(r'\.((?:\\[0-9a-fA-F]{1,6}\s?|\\.|[-\w\u0080-\uffff])+)')
URLS = re.compile(r'''url\(\s*(?:"((?:\\.|[^"\\])*)"|'((?:\\.|[^'\\])*)'|([^)]*?))\s*\)''', re.I)


def parse_native(text, contents=()):
    constants = []
    for match in re.finditer(r'pub const (\w+): Color32 = (.*?);([^\n]*)', text):
        constants.append({'name': match[1], 'expression': match[2],
                          'source_tokens': re.findall(r'--[\w-]+', match[3])})
    icons = []
    for match in re.finditer(r"Icon::(\w+)\s*=>\s*'\\u\{([\da-fA-F]+)\}'.*?//\s*\.([\w-]+)", text):
        icons.append({'variant': match[1], 'codepoint': f'U+{int(match[2],16):04X}', 'css_class': match[3]})
    for match in re.finditer(r'Icon::(\w+)\s*=>\s*"([\w-]+)"', text):
        codepoints = {entry['codepoints'][0] for entry in contents if len(entry['codepoints']) == 1
                      and any(re.search(r'\.' + re.escape(match[2]) + r'::?(?:before|after)$', selector)
                              for selector in entry['selectors'])}
        if len(codepoints) != 1:
            raise ValueError('Native icon must have one original terminal glyph: ' + match[2])
        icons.append({'variant': match[1], 'codepoint': next(iter(codepoints)), 'css_class': match[2]})
    enum = re.search(r'pub enum Icon\s*\{([^}]+)\}', text)
    return {'color_constants': constants, 'icons': icons,
            'icon_enum_variants': re.findall(r'\b(\w+)\s*,', enum[1]) if enum else []}


def theme_runtime():
    """Record narrow, source-checked JS anchors, without evaluating bundled code."""
    path = ROOT / 'pc-dist/compact-app-pc.e08d0d44f38873747a6b.js'
    text = path.read_text()
    patterns = {
        'palette_enum': r'\{\s*DARK:\s*"dark",\s*LIGHT:\s*"light",\s*CUSTOM:\s*"custom"\s*\}',
        'selection_enum': r'\{\s*LIGHT:\s*0,\s*DARK:\s*1,\s*SYSTEM:\s*2,\s*CUSTOM:\s*3\s*\}',
        'system_dark_query': r'"\(prefers-color-scheme: dark\)"',
    }
    anchors = []
    for name, pattern in patterns.items():
        match = re.search(pattern, text)
        if match is None:
            raise ValueError(f'Missing original theme runtime anchor: {name}')
        anchors.append({'name': name, 'offset': match.start(), 'text': match[0]})
    return {'source': digest(path), 'anchors': anchors,
            'limitation': 'Anchors establish enum/query facts, not runtime behavior or complete JS asset coverage.'}


def build_inventory():
    sources, properties, contents, families, faces, assets, font_shorthands = [], [], [], [], [], [], []
    referenced_tokens = set()
    for path in sorted((ROOT / 'pc-dist').glob('*.css')):
        source = dict(digest(path))
        css = path.read_text()
        rules = scan_css(css)
        source['rule_count'] = len(rules); sources.append(source)
        for rule in rules:
            context = {key: rule[key] for key in ('rule', 'offset', 'selectors', 'at_rules')}
            context['source'] = source['path']
            for declaration in rule['declarations']:
                referenced_tokens.update(re.findall(r'\bvar\(\s*(--[\w-]+)', STRINGS.sub('', declaration['value'])))
                entry = dict(context, **declaration)
                if declaration['name'].startswith('--'):
                    properties.append(entry)
                if declaration['name'] == 'content':
                    strings = [unescape_css(m[2]) for m in STRINGS.finditer(declaration['value'])]
                    selectors = STRINGS.sub('', ' '.join(rule['selectors']))
                    entry.update({'classes': sorted(set(unescape_css(m[1]) for m in CLASSES.finditer(selectors))),
                                  'decoded_strings': strings,
                                  'codepoints': [f'U+{ord(ch):04X}' for value in strings for ch in value]})
                    contents.append(entry)
                if declaration['name'] == 'font-family':
                    families.append(entry)
                if declaration['name'] == 'font':
                    font_shorthands.append(entry)
            if rule['selectors'] == ['@font-face']:
                faces.append(dict(context, declarations=rule['declarations']))
        for match in URLS.finditer(without_comments(css)):
            raw = unescape_css(next(group for group in match.groups() if group is not None))
            entry = {'source': source['path'], 'offset': match.start()}
            if raw.startswith('#'):
                entry.update(kind='fragment', url=raw)
            elif raw.startswith('data:'):
                entry.update(kind='inline', bytes=len(raw.encode()), sha256=hashlib.sha256(raw.encode()).hexdigest())
            elif re.match(r'^[\w+.-]+:', raw) or raw.startswith('//'):
                entry.update(kind='external', url=raw)
            else:
                local = (path.parent / raw.split('?', 1)[0].split('#', 1)[0]).resolve()
                entry.update(kind='local', url=raw)
                if local.is_relative_to(ROOT) and local.is_file():
                    entry.update(asset=digest(local))
                else:
                    entry.update(missing=True)
            assets.append(entry)
    native_path = ROOT / 'rust-client/src/ui/visuals.rs'
    native = parse_native(native_path.read_text(), contents)
    native['source'] = digest(native_path)
    native['embedded_assets'] = []
    for match in re.finditer(r'include_bytes!\(\s*"([^"]+)"', native_path.read_text()):
        native['embedded_assets'].append(digest((native_path.parent / match[1]).resolve()))
    theme_path = ROOT / 'rust-client/src/theme.rs'
    native['semantic_color_roles'] = []
    if theme_path.is_file():
        native['theme_source'] = digest(theme_path)
        native['semantic_color_roles'] = [
            {'role': match[1], 'source_tokens': [match[2]]}
            for match in re.finditer(r'(\w+):\s*color\("(--[\w-]+)"\)', theme_path.read_text())]
        for match in re.finditer(r'(\w+):\s*\[([^]]*)\]', theme_path.read_text()):
            source_tokens = re.findall(r'color\("(--[\w-]+)"\)', match[2])
            if source_tokens:
                native['semantic_color_roles'].append({'role': match[1], 'source_tokens': source_tokens})
    tokens = sorted({entry['name'] for entry in properties})
    bound_tokens = sorted({token for entry in native['color_constants'] + native['semantic_color_roles'] for token in entry['source_tokens']})
    classes = sorted({name for entry in contents for name in entry['classes'] if name.startswith('fa-')})
    bound_classes = sorted({entry['css_class'] for entry in native['icons']})
    registry_path = ROOT / 'rust-client/src/theme/catalog_data.json'
    registry_classes = []
    if registry_path.is_file():
        registry = json.loads(registry_path.read_text())
        registry_classes = sorted(row['class'] for row in registry['glyphs'])
        native['icon_registry'] = {'source': digest(registry_path), 'glyph_count': len(registry_classes),
                                   'fonts': registry['fonts']}
    mismatches = [icon for icon in native['icons'] if not any(icon['css_class'] in entry['classes'] and icon['codepoint'] in entry['codepoints'] for entry in contents)]
    return {'schema_version': 1,
            'scope': 'All pc-dist/*.css, pc-dist/fonts files, literal CSS url references, and native visuals.rs bindings. Raw facts; no CSS cascade evaluation or runtime completeness claim. Other JS/assets require separate inventory.',
            'css_sources': sources, 'custom_properties': properties, 'content_rules': contents,
            'theme_runtime': theme_runtime(),
            'font_family_rules': families, 'font_faces': faces,
            'font_shorthand_rules': font_shorthands,
            'font_files': [digest(path) for path in sorted((ROOT / 'pc-dist/fonts').iterdir()) if path.is_file()],
            'visual_asset_references': assets, 'native': native,
            'coverage': {'unique_custom_properties': len(tokens), 'native_token_names': bound_tokens,
                         'referenced_token_names': sorted(referenced_tokens),
                         'referenced_without_css_declaration': sorted(referenced_tokens - set(tokens)),
                         'pending_token_names': sorted(set(tokens) - set(bound_tokens)),
                         'unique_fa_classes': len(classes), 'native_icon_classes': bound_classes,
                         'registry_glyph_classes': registry_classes,
                         'fa_classes_without_registry_glyph': sorted(set(classes)-set(registry_classes)),
                         'native_icon_variants_without_binding': sorted(set(native['icon_enum_variants']) - {entry['variant'] for entry in native['icons']}),
                         'pending_fa_classes': sorted(set(classes) - set(bound_classes)),
                         'native_icon_codepoint_mismatches': mismatches,
                         'limitations': ['Font family rules are raw selector associations, not resolved inheritance.',
                                         'Tokens referenced in comments are traceability, not verified native state coverage.',
                                         'pending_token_names and pending_fa_classes mean no explicit semantic Palette/Icon binding; full catalogs remain available separately.',
                                         'System selection and custom persistence require separate JavaScript/runtime mapping.',
                                         'content_rules include decorative text and empty pseudo-elements, not only icons.']}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true', help='Fail if the checked-in inventory differs')
    args = parser.parse_args()
    inventory = build_inventory()
    encoded = json.dumps(inventory, ensure_ascii=True, indent=2, sort_keys=True) + '\n'
    output = ROOT / OUTPUT
    if args.check:
        if not output.is_file() or output.read_text() != encoded:
            raise SystemExit('UI inventory is stale; run scripts/rust/inventory-ui-assets.py')
    else:
        output.write_text(encoded)
    print(json.dumps({'css_sources': len(inventory['css_sources']), 'font_files': len(inventory['font_files']),
                      'custom_property_declarations': len(inventory['custom_properties']),
                      'content_rules': len(inventory['content_rules']), 'font_faces': len(inventory['font_faces']),
                      'native_icons': len(inventory['native']['icons']),
                      'unique_fa_classes': inventory['coverage']['unique_fa_classes'],
                      'output': str(OUTPUT)}, sort_keys=True))


if __name__ == '__main__':
    main()
