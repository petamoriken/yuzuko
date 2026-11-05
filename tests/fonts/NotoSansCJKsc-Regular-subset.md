# Noto Sans CJK SC subset

`NotoSansCJKsc-Regular-subset.otf` is the one-glyph subset used by
`tests/canvas_lang_test.ts`. It keeps the `locl` feature for U+9AA8 (骨).
The license is `NotoSansCJKsc-Regular-subset-LICENSE.txt` (SIL Open Font
License 1.1). The copyright is in the font name table: © 2014-2021 Adobe
(http://www.adobe.com/).

The full font is
<https://github.com/notofonts/noto-cjk/releases/download/Sans2.004/08_NotoSansCJKsc.zip>
(`NotoSansCJKsc-Regular.otf`, sha256
`2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b`).

```sh
fonttools subset NotoSansCJKsc-Regular.otf \
  --output-file=NotoSansCJKsc-Regular-subset.otf \
  --unicodes=U+9AA8 \
  --layout-features='*' \
  --glyph-names \
  --symbol-cmap \
  --legacy-cmap \
  --notdef-glyph --notdef-outline \
  --recommended-glyphs \
  --name-IDs='*' \
  --no-hinting \
  --desubroutinize
```

The committed file is the subset from
<https://github.com/linebender/parley/pull/756>
(sha256 `9703e45aab1a7310db60506684d141f5293a18bf2788988f3f674ff938a58a9c`).
