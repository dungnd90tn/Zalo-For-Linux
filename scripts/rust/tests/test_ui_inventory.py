import importlib.util
from pathlib import Path
import unittest
import struct


SPEC = importlib.util.spec_from_file_location(
    'ui_inventory', Path(__file__).resolve().parents[1] / 'inventory-ui-assets.py')
inventory = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(inventory)
GEN_SPEC = importlib.util.spec_from_file_location(
    'ui_catalog_generator', Path(__file__).resolve().parents[1] / 'generate-ui-catalog.py')
generator = importlib.util.module_from_spec(GEN_SPEC)
GEN_SPEC.loader.exec_module(generator)


class UiInventoryTests(unittest.TestCase):
    def test_nested_context_aliases_and_raw_values_survive(self):
        css = r'''/* } fake { --fake: red; */
        @media (prefers-color-scheme: dark) {
            @supports (color: var(--palette)) {
                :root, [data-label="a,b"] { --palette: color-mix(in srgb, red, blue); }
                .fa-a:after,.fa-b:hover:before {
                    content: "\e001"; --url: url("data:image/svg+xml;a:b,{c}");
                    --literal: hello : world;
                }
            }
        }'''
        rules = inventory.scan_css(css)
        self.assertEqual(len(rules), 2)
        self.assertEqual(rules[0]['selectors'], [':root', '[data-label="a,b"]'])
        self.assertEqual(rules[1]['at_rules'], [
            '@media (prefers-color-scheme: dark)', '@supports (color: var(--palette))'])
        declarations = {item['name']: item['value'] for item in rules[1]['declarations']}
        self.assertEqual(declarations['--literal'], 'hello : world')
        self.assertEqual(declarations['--url'], 'url("data:image/svg+xml;a:b,{c}")')
        self.assertNotIn('--fake', declarations)
        self.assertEqual([inventory.unescape_css(m[2]) for m in inventory.STRINGS.finditer(declarations['content'])], ['\ue001'])

    def test_comments_escaped_quotes_and_font_faces(self):
        css = r'''@charset "UTF-8"; @font-face {font-family:'Zalo';src:url(font.woff2?#fragment)}
        .fa-label:after { content: "a\"b/*not a comment*/;c"; }'''
        rules = inventory.scan_css(css)
        self.assertEqual(rules[0]['selectors'], ['@font-face'])
        self.assertEqual(rules[0]['declarations'][1]['value'], 'url(font.woff2?#fragment)')
        raw = rules[1]['declarations'][0]['value']
        self.assertEqual([inventory.unescape_css(m[2]) for m in inventory.STRINGS.finditer(raw)], ['a"b/*not a comment*/;c'])

    def test_css_hex_escapes_and_class_aliases(self):
        self.assertEqual(inventory.unescape_css(r'\000041 B\e001\0\110000'), 'AB\ue001\ufffd\ufffd')
        selector = r'.fa-\61 lias:after,.fa-second:hover:before'
        self.assertEqual([inventory.unescape_css(m[1]) for m in inventory.CLASSES.finditer(selector)], ['fa-alias', 'fa-second'])

    def test_malformed_source_is_not_silently_partial_inventory(self):
        for css in ('a{color:red', 'a{color:"red}', '/* incomplete', 'a}b{color:red}'):
            with self.subTest(css=css), self.assertRaises(ValueError):
                inventory.scan_css(css)

    def test_native_bindings_keep_source_names_and_codepoints(self):
        parsed = inventory.parse_native(r'''
            pub const BLUE: Color32 = Color32::from_rgb(0, 104, 255); // --accent-blue
            pub enum Icon { Chat, Smile, }
            Icon::Smile => '\u{ea5c}', // .fa-chatbar-emoji
        ''')
        self.assertEqual(parsed['icon_enum_variants'], ['Chat', 'Smile'])
        self.assertEqual(parsed['icons'], [{'variant': 'Smile', 'codepoint': 'U+EA5C', 'css_class': 'fa-chatbar-emoji'}])
        self.assertEqual(parsed['color_constants'][0]['source_tokens'], ['--accent-blue'])

    def test_native_class_binding_uses_terminal_pseudo_not_ancestor(self):
        contents = [{'selectors': ['.fa-checkbox tick:after'], 'codepoints': ['U+E001']},
                    {'selectors': ['.fa-real:after'], 'codepoints': ['U+E002']}]
        parsed = inventory.parse_native('Icon::Chat => "fa-real",', contents)
        self.assertEqual(parsed['icons'][0]['codepoint'], 'U+E002')
        with self.assertRaises(ValueError):
            inventory.parse_native('Icon::Chat => "fa-checkbox",', contents)
        contents.append({'selectors': ['.fa-real:before'], 'codepoints': ['U+E003']})
        with self.assertRaises(ValueError):
            inventory.parse_native('Icon::Chat => "fa-real",', contents)

    def test_real_font_cmap_supports_bmp_supplementary_and_missing_glyph(self):
        def font(tables):
            header = struct.pack('>IHHHH', 0x10000, 1, 16, 0, 0)
            offset = 4 + len(tables) * 8
            records, bodies = [], []
            for index, table in enumerate(tables):
                records.append(struct.pack('>HHI', 3, 10 if index else 1, offset))
                bodies.append(table); offset += len(table)
            cmap = struct.pack('>HH', 0, len(tables)) + b''.join(records + bodies)
            return header + b'cmap' + struct.pack('>III', 0, 28, len(cmap)) + cmap
        bmp = struct.pack('>HHHHHHH', 4, 32, 0, 4, 4, 1, 0)
        bmp += struct.pack('>HHHHHHHHH', 0xea5c, 0xffff, 0, 0xea5c, 0xffff,
                           (17 - 0xea5c) & 0xffff, 1, 0, 0)
        supplementary = struct.pack('>HHIII', 12, 0, 28, 0, 1) + struct.pack('>III', 0x1f600, 0x1f601, 23)
        data = font([bmp, supplementary])
        self.assertEqual(generator.cmap_glyph(data, 0xea5c), 17)
        self.assertEqual(generator.cmap_glyph(data, 0x1f600), 23)
        self.assertEqual(generator.cmap_glyph(data, 0x1f601), 24)
        self.assertEqual(generator.cmap_glyph(data, 0xe000), 0)
        original = next((inventory.ROOT / 'pc-dist/fonts').glob('zalo-font.*.ttf')).read_bytes()
        self.assertGreater(generator.cmap_glyph(original, 0xea5c), 0)


if __name__ == '__main__':
    unittest.main()
