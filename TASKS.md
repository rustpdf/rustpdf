# Acompanhamento de tarefas — rust-pdf

> Baseado em [`project.md`](project.md). Estado em **2026-06-25**.
> Legenda: ✅ feito · 🟡 parcial · ⏳ pendente (fase futura)

## Resumo

| Fase | Escopo | Estado |
|------|--------|--------|
| Fase 0 | Fundação e tooling | ✅ completa |
| Fase 1 | Modelo COS + serialização | ✅ completa |
| Fase 2 | Páginas, content stream, gráficos vetoriais | ✅ completa |
| Fase 3 | Texto e fontes (o coração) | ✅ núcleo completo (3A–3D, 3E.3/3E.4, 3F); 3E.1/3E.2/3D.3 parciais |
| Fase 4 | Imagens | ✅ completa (JPEG, PNG, paleta, alpha/SMask, 16-bit) |
| Fase 5 | Parser de PDF existente | ✅ núcleo completo (5.1–5.8, 5.10; 5.9 RC4+AESv2+**AES-256/R6**) |
| Fase 6 | Manipulação (Tier 2) | ✅ completo (6.1–6.8: **object streams + xref stream**, **dedupe**, **incremental update genérico**, **AcroForm 6.7 com `/AP`**) |
| Fase 7 | Diferenciadores (Tier 3) | 🟡 7.1–7.6 ✅ (assinatura, PAdES/LTV, cripto **+AES-256 + CSPRNG**, **PDF/A-1b/2b/2a/3b/3a**, **PDF/UA-1 com tags semânticas + listas/caption/Headers**, layout); só 7.7 (HTML→PDF) — fora de escopo |

**Licenciamento (corporativo):** crate `license` (Ed25519) + `pdf::activate_license`
bloqueia PDF/A, assinatura/PAdES, criptografia e acessibilidade sem licença
válida/assinada/não-expirada. Ver `docs/LICENSING.md`.

Métricas atuais: 157 testes verdes · clippy `-D warnings` limpo · fmt limpo ·
**PDF/A-1b/2b/2a/3b/3a e PDF/UA-1 validados pelo veraPDF**; **AES-256/R6, object
streams, dedupe, incremental update e AcroForm validados pelo `qpdf`**;
assinatura/PAdES validados por `pdfsig`/`openssl`.

> **Itens pendentes consolidados:** ver [PENDING.md](PENDING.md). Resumo rápido:
> Knuth–Plass (3D.3), Árabe/Indic e2e (3E.1/3E.2),
> edição de forms existentes com `/AP`, conversão PDF lido → PDF/A, e TSA/OCSP
> via rede. (AES-256, object streams, dedupe, incremental update, PDF/A-1b/A-3,
> AcroForm, listas/caption/Headers e hardening RNG — feitos em 2026-06-25.)

---

## Fase 0 — Fundação e tooling

- [x] **0.1** Workspace Cargo multi-crate — `cargo build` compila todos os crates
- [x] **0.2** CI (build, test, clippy, fmt, deny) — `.github/workflows/ci.yml`
- [x] **0.3** Corpus dourado — 34 catalogados (`corpus/catalog.json`), 23 gerados; categorias dependentes de fase marcadas `pending`
- [x] **0.4** Harness de validação externa — `testkit::validate` (qpdf/mutool/verapdf, degrada se ausente)
- [x] **0.5** Regressão visual — `testkit::render_to_png` + `visual_diff` (diff perceptual)
- [x] **0.6** Decisão de porte em ADR — `docs/adr/0001-porting-strategy.md` (+ 0002 concorrência)
- [x] **0.7** Crate `ffi` + `cbindgen` gerando header — `include/pdf.h` via `build.rs`
- [x] **0.8** Binding de referência Python (ctypes) que carrega a cdylib — `bindings/python/`
- [x] **0.9** `FFI_RULES.md` + checklist de revisão — `docs/FFI_RULES.md` + `.github/pull_request_template.md`
- [x] **0.10** Matriz de build por plataforma — job `matrix` no CI (linux/macos/win × x86_64/arm64)

## Fase 1 — Modelo de objetos COS + serialização

- [x] **1.1** Tipos COS (Null, Bool, Integer, Real, Name, String, Array, Dict, Stream, Reference)
- [x] **1.2** Serializador objeto→bytes por tipo (round-trip canônico)
- [x] **1.3** Escape correto (literais `\(`, hex `<...>`, nomes `#XX`, parênteses aninhados, não-ASCII)
- [x] **1.4** Stream object com `/Length` (direto e indireto)
- [x] **1.5** Document model (header, body, xref clássico, trailer, `%%EOF`)
- [x] **1.6** 🏁 Milestone: PDF de 1 página em branco — passa `qpdf --check` + `mutool clean`
- [x] **1.7** 🏁 Dogfood da fronteira — Python (ctypes) gera PDF **byte-idêntico** à API Rust

## Fase 2 — Páginas, content stream e gráficos vetoriais

- [x] **2.1** Page tree (`/Pages`, `/Kids`, `/Count`), Page, MediaBox, coordenadas
- [x] **2.2** Content stream builder (operadores)
- [x] **2.3** Estado gráfico (`q`/`Q`, `cm`, `w`)
- [x] **2.4** Cores device (`rg`/`RG`, `g`/`G`, `k`/`K`)
- [x] **2.5** Path (`m`, `l`, `c`, `v`, `y`, `re`, `h`)
- [x] **2.6** Pintura (`S`, `f`, `f*`, `B`, `n`) + clipping (`W`/`W*`)
- [x] **2.7** 🏁 Milestone: PDF com linhas/retângulos/curvas coloridos — validado e renderizado

---

## Fase 3 — Texto e fontes (o coração) ✅

Crate `fonts` (ttf-parser, rustybuzz, subsetter, unicode-bidi) + integração no
`pdf` (módulos `font`/`text`/`paragraph`). Caminho universal **Type0/CIDFontType2
+ Identity-H + subset**, validado com `qpdf`, `mutool` e `pdftotext`.

### Sub-épico A — Fontes simples Latin
- [x] **3A.1** Parse TTF/OTF (head/hhea/hmtx/maxp/cmap/name/OS-2/glyf) via `ttf-parser`
- [x] **3A.2** Embutir fonte: `FontFile2` (TrueType) embutido e verificado no `mutool info`
- [x] **3A.4** Operadores de texto (`BT`/`ET`, `Tf`, `Td`/`Tm`, `Tj`/`TJ`, `Tc`, `Tw`, `Tz`, `TL`, `Ts`, `Tr`)
- [x] **3A.5** 🏁 Milestone "Hello World" embutido — `pdftotext`/`mutool` extraem corretamente
- [~] **3A.3** Dict de fonte simples + `WinAnsiEncoding` — **superado** pelo caminho Type0 universal (mesmo critério de extração atendido; single-byte simple font não foi implementado separadamente)

### Sub-épico B — Unicode / Type0 / CID ✅
- [x] **3B.1** Type0 + CIDFontType2 + `Identity-H`
- [x] **3B.2** `CIDToGIDMap` (Identity)
- [x] **3B.3** `W` array de larguras CID (em glyph space 1000/em)
- [x] **3B.4** `ToUnicode` CMap — acentos (é, ñ, ç) extraem corretamente

### Sub-épico C — Subsetting ✅
- [x] **3C.1/3C.2** Coletar glyphs, remapear GIDs, reconstruir (via `subsetter`)
- [x] **3C.3** 🏁 Milestone subset — Roboto 515KB → PDF ~8KB (>70% menor), renderiza e extrai

### Sub-épico D — Shaping e quebra de linha (Latin)
- [x] **3D.1** Shaping via `rustybuzz` (kerning GPOS/kern, ligaduras) — adjustes via `TJ`
- [x] **3D.2** Quebra de linha gulosa (no `paragraph`)
- [ ] **3D.3** (Opcional v2) Knuth-Plass — não implementado (opcional)

### Sub-épico E — Scripts complexos
- [~] **3E.1** Árabe — shaping funciona via rustybuzz; integração RTL não validada ponta a ponta
- [~] **3E.2** Devanagari/Indic — idem (shaper suporta; sem validação dedicada)
- [x] **3E.3** BiDi via `unicode-bidi` (`reorder_runs`, testado LTR/RTL misto)
- [x] **3E.4** CJK — renderiza e extrai (testado com fonte do sistema; subset de fonte grande)

### Sub-épico F — L2 básico (parágrafo) ✅
- [x] **3F.1** `paragraph(texto)` com quebra automática dentro de uma caixa
- [x] **3F.2** Alinhamento (left/right/center/justify) + leading
- [x] **3F.3** Estilo inline (fonte/tamanho/cor) dentro do parágrafo
- [x] **3F.4** 🏁 Milestone DX — heading + parágrafo ponta a ponta (exemplo `report.rs`)

## Fase 4 — Imagens ✅

Crate `images` (`png` + `flate2`) + integração no `pdf` (módulo `image`, op `Do`
no `graphics`). Validado com `qpdf`/`mutool` e render com amostragem de pixels.

- [x] **4.1** JPEG: embed direto via `DCTDecode` (parse do SOF; bytes verbatim, sem recomprimir)
- [x] **4.2** Image XObject + operador `Do` (com `cm` para posicionar/escalar)
- [x] **4.3** PNG: decodifica (`png`), recodifica `FlateDecode` (RGB/Gray opacos)
- [x] **4.4** PNG paleta → `Indexed`; alpha (RGBA/GA/`tRNS`) → `SMask`; 16-bit
  - Nota: CMYK JPEG aplica `/Decode` invertido quando há marcador Adobe; paleta+`tRNS` mapeado para bpc=8 (sub-byte fica opaco)

## Fase 5 — Parser de PDF existente ✅

Crate `parser` (`flate2`, `aes`, `cbc`, `md-5`) com módulos `lexer`/`object`/
`filters`/`xref`/`crypt`/`reader`. Testes usam fixtures commitados em
`crates/parser/tests/fixtures/` (geradas com qpdf) — auto-contidos, sem spawnar
ferramentas externas (o sandbox de teste não enxerga o qpdf).

- [x] **5.1** Tokenizer/lexer (não dá panic em entrada malformada)
- [x] **5.2** Parser de objetos indiretos (refs, dict, array, stream)
- [x] **5.3** xref clássico + trailer + `/Root` + `/Info` (page tree)
- [x] **5.4** Filtros: `FlateDecode` (+ predictors PNG/TIFF), `LZWDecode`, `ASCIIHexDecode`, `ASCII85Decode`, `RunLengthDecode`
- [x] **5.5** xref streams (cross-reference streams, PDF 1.5+)
- [x] **5.6** Object streams (objetos comprimidos)
- [x] **5.7** Lineares/híbridos (segue `/XRefStm` e `/Prev`)
- [x] **5.8** Recuperação: reconstrói xref por scan de `obj`; acha catálogo
- [x] **5.9** Cifrado (standard handler): RC4 (R2/R3), AESv2 (R4) e **AESv3 (V5/R6, AES-256)** ✅ validados (Algorithm 2.A/2.B; fixture `enc_aes256.pdf`)
- [x] **5.10** 🏁 Milestone round-trip — base, modern (xref/objstm) e cifrados → re-parse + `qpdf --check` passam (verificado externamente)

## Fase 6 — Manipulação (Tier 2) ✅ (núcleo)

Crate `pdf`, módulos `edit` (`EditableDoc`) e `extract`. `EditableDoc` carrega
via `parser`, achata a árvore de páginas, edita e reescreve via `writer`. Saídas
validadas externamente com `qpdf --check`, `mutool` e `pdftotext`.

- [x] **6.1** Merge — renumera objetos, mescla page trees, N PDFs → 1
- [x] **6.2** Split / extrair páginas com dependências (reachability + renumber compacto)
- [x] **6.3** Rotacionar (`/Rotate`), reordenar, deletar páginas
- [x] **6.4** Extração de texto (content stream → glyphs → Unicode via `ToUnicode`; infere espaços/linhas) — recupera parágrafos justificados; foco RAG
- [x] **6.5** Metadados: `/Info` + XMP (`/Metadata`) ler/escrever (round-trip)
- [x] **6.6** Overlay/watermark (content stream sobre a página, sem corromper)
- [x] **6.7** AcroForm: **autoria** `text_field`/`checkbox`/`radio_group`/`dropdown` com **appearance streams `/AP` gerados** (Helvetica/ZapfDingbats), **checkbox/radio/choice** e **nomes hierárquicos** (`a.b.c`); `qpdf` lê fullname/valor. Edição de forms existentes com `/AP` → pendente (PENDING.md)
- [x] **6.8** Otimização: remove objetos não-usados + recomprime streams + **object streams (`/ObjStm`) + `/XRef` stream na escrita** (`optimize`/`compact`) + **dedupe de objetos byte-idênticos** + **incremental update genérico** (`to_bytes_incremental`); tudo validado por `qpdf`

> Limitações de 6.4 (extração): a ordem de leitura é por fluxo do content stream
> (heurística de coluna/linha simples); layouts multi-coluna complexos podem sair
> fora de ordem. Ver PENDING.md.

## Fase 7 — Diferenciadores (Tier 3) 🟡

Tier explicitamente "escopo leve, vai mudar" no `project.md`. Entregue o que é
**validável agora**; o resto está detalhado em [PENDING.md](PENDING.md).

- [x] **7.3** Criptografia na escrita: standard handler RC4-128 (V2/R3), AES-128
  (V4/R4, `AESV2`) e **AES-256 (V5/R6, `AESV3`)** + flags de permissão
  (`Permissions`); abre com senha vazia, respeita permissões. R6 calcula
  `/U`/`/UE`/`/O`/`/OE`/`/Perms` (Algorithms 8–10). **IVs/salts/file-key vêm de
  CSPRNG (`getrandom`)** — cada cifragem é única. Validado: `qpdf` reconhece
  cifra+permissões e abre com senha de **usuário e proprietário**, `pdftotext`
  decifra, e o **próprio parser** re-decifra (round-trip).
- [x] **7.6** Engine de layout (`Report`/`Table`): tabelas com células que
  quebram, header/footer corrente, numeração de página, paginação automática,
  cabeçalho de tabela repetido entre páginas. Estende o L2 (3F). Validado com
  `qpdf`/`mutool`/render.
- [x] **7.1** Assinatura digital (`pdf::sign`): incremental update + `ByteRange`
  + PKCS#7/CMS detached (`adbe.pkcs7.detached`, SHA-256, RSA via RustCrypto
  `cms`/`rsa`). Inclui **assinatura visível** (appearance Helvetica),
  **múltiplas assinaturas** e **cadeia de certificados**. Validado: **`pdfsig`
  → todas "Signature is Valid"** (2 assinaturas), `qpdf --check` limpo, parser
  lê o update incremental. Pendentes: PAdES/LTV/timestamp e validação no
  Acrobat (ver PENDING.md).
- [x] **7.2** PAdES + LTV. **B-B** (`SignOptions.pades`): `ETSI.CAdES.detached`
  + ESS `signing-certificate-v2`. **B-LT** (`add_dss`): Document Security Store
  `/DSS` com `/Certs` e `/CRLs`. **B-LTA** (`timestamp`): document timestamp
  `/DocTimeStamp` `ETSI.RFC3161` (token RFC 3161 real, assinado por um TSA).
  Validado: `pdfsig` (assinatura válida + lista o timestamp), **`openssl cms
  -verify` → "CMS Verification successful"** no token, `openssl asn1parse`
  confirma TSTInfo/`id-smime-ct-TSTInfo`, `qpdf --check` limpo.
  **Pendente (precisa de infra externa):** TSA **confiável** via rede (aqui o TSA
  é auto-emitido/offline), busca automática de **OCSP/CRL** (hoje fornecidos pelo
  chamador), `/VRI` por assinatura no DSS, e validação num verificador PAdES
  (DSS/Adobe). Ver PENDING.md.
- [x] **7.4** PDF/A — **níveis 1b/2b/2a/3b/3a** via `Document::pdfa()`/`pdfa_a()`/
  `pdfa_with(PdfaLevel)`: `OutputIntent` + ICC sRGB embutido + XMP (sincronizado
  com `/Info`, `pdfaid:part` 1/2/3) + `/ID`. **A-1b**: header PDF 1.4, `/CIDSet`
  (lido do programa de subset), sem object streams. **A-3**: anexos via
  `attach_file` → `/EmbeddedFile`+`/AFRelationship`+`/AF`+`/Names /EmbeddedFiles`.
  **Validado: `verapdf` → isCompliant=true para 1b, 2b, 2a, 3b e 3a.** Pendente:
  conversão de PDF *lido* → PDF/A.
- [x] **7.5** Tagged PDF / acessibilidade (`Document::tagged()` / `pdfa_a()`):
  structure tree **aninhado** (`tagtree.rs`, keyed por path com tag → sem colisão
  lista/tabela), marked content, `ParentTree`, `/MarkInfo`, `/Lang`,
  `ViewerPreferences`, XMP `pdfaid`+`pdfuaid`. **Tags semânticas:** `/H1`–`/H6`,
  `/Figure`+`/Alt` UTF-16BE, `/Table`/`/TR`/`/TH`/`/TD` com `/Scope` **e**
  `/Headers`↔`/ID`, **listas `/L`/`/LI`/`/LBody`** (`Report::list`) e **`/Caption`**.
  **`/Span` inline por-run** (`TextObject::show_span` → marked content aninhado;
  elemento Span filho do P via `/K` misto). **Validado: `verapdf -f 2a` (PDF/A-2a)
  e `-f ua1` (PDF/UA-1)** no doc básico, rico, fino e com span inline.
- [ ] **7.7** HTML/CSS→PDF — "produto à parte" (motor de layout de browser).
  Fora de escopo; pendente.
- [x] **7.8** **Rasterização de página** (renderizar página → imagem). Novo crate
  **`render`**: interpretador de content stream nativo sobre `tiny-skia`
  (BSD-3, puro Rust). Cobre vetores (preencher/traçar/clip, nonzero/even-odd,
  dash/cap/join), **texto com outlines reais** (Type0/Identity-H + fontes
  simples WinAnsi/Differences; fallback Roboto p/ não-embutidas via
  `ttf-parser`), **imagens** (XObject + inline: amostras cruas por colorspace+
  bits, JPEG via `jpeg-decoder`, `/SMask`, `/ImageMask`, `/Decode`), **espaços
  de cor** (Gray/RGB/CMYK/ICCBased-por-N/Indexed/Separation/DeviceN com
  funções tipo 0/2/3/4), **Form XObjects** (Matrix+BBox+recursão), **ExtGState**
  (`ca`/`CA`/`BM`), **sombreamentos** axial (tipo 2) e radial (tipo 3), e
  `/Rotate`/`/CropBox`. API: `pdf::render_page_to_png`/`render_page_rgba`(`_with`)
  + FFI `pdf_render_page_to_png`/`pdf_page_count`. Testado ponta-a-ponta
  (vetores posicionados, glifos, imagem RGB, CMYK) + unit tests do avaliador de
  funções + **regressão visual vs mutool** (`tests/visual_regression.rs`:
  testkit `render_to_png` + diff perceptual; corpus even-odd/CTM/Unicode/alpha
  <0.0003, vetores/texto ~0.001). **Licenciado como feature Pro**
  (`Feature::Rendering`; gate no wrapper `pdf::render_page_*`). Lacunas conhecidas
  (mesh shadings 4–7, tiling patterns, CCITT/JPX/JBIG2, soft mask por
  luminosidade, FontFile Type1) em `PENDING.md`.

---

## Definition of Done (toda tarefa — `project.md` §10)

- [ ] Testes unitários cobrindo casos-limite
- [ ] Passa nos validadores externos aplicáveis
- [ ] Regressão visual (se gera conteúdo renderizável)
- [ ] Round-trip no corpus dourado (se toca parser/writer)
- [ ] Núcleo permanece `Send` (sem `Rc`/`RefCell`; cache global `Sync`)
- [ ] Tipos do núcleo não vazam pela borda (só `ffi`)
- [ ] Licença de nova dependência registrada em `LICENSES.md`
