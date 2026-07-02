# Itens pendentes — rust-pdf

> Registro consolidado de tudo que ficou **parcial** ou **adiado** nas Fases 0–6,
> para não se perder. Cada item aponta a fase/critério do [`project.md`](project.md)
> e o estado atual. Atualizado em **2026-06-25** (após Fase 7 + AES-256, object
> streams na escrita e tags semânticas de acessibilidade).

Legenda: 🟡 parcial (implementado em parte) · ⏳ adiado (não iniciado)

---

## Fase 3 — Texto e fontes

- ⏳ **3D.3 Knuth–Plass** (justificação ótima). Hoje a quebra de linha é gulosa
  (`crates/pdf/src/paragraph.rs`). Era marcado "opcional v2" no `project.md`.
- 🟡 **3E.1 Árabe** / 🟡 **3E.2 Devanagari/Indic** — o *shaping* funciona via
  `rustybuzz` (`crates/fonts/src/shape.rs`) e o caminho Type0 embute qualquer
  glifo, mas **não há validação ponta-a-ponta** (RTL + reordenação Indic dentro
  do `paragraph`/`text`) nem fixtures de baseline HarfBuzz. BiDi (3E.3) e CJK
  (3E.4) estão feitos e testados.
- Nota: caminho de fonte **simples WinAnsi single-byte (3A.3)** foi
  deliberadamente substituído pelo Type0 universal — não é uma pendência, é uma
  decisão de design (mesmo critério de extração atendido).

## Fase 5 / 7.3 — Criptografia

- ✅ **AES-256 (V5 / R6, AESv3)** — **feito** na **leitura** (5.9) e na **escrita**
  (7.3). Leitura: derivação Algorithm 2.A/2.B (SHA-256/384/512 + AES-128),
  `/UE`/`/OE`, AES-256-CBC; valida o fixture `enc_aes256.pdf` do qpdf. Escrita:
  `EditableDoc::encrypt_with(Encryption::Aes256, …)` calcula `/U`/`/UE`/`/O`/`/OE`
  /`/Perms` (Algorithms 8–10); `qpdf --check` aceita e abre com senha de usuário
  **e** de proprietário. Dep `sha2` adicionada ao `parser`.
- ✅ **IVs / file key / salts na escrita usam CSPRNG** (`getrandom`) — cada
  cifragem é única (PDF spec). A saída cifrada deixa de ser byte-reproduzível,
  mas o invariante de determinismo só vale para PDFs **não** cifrados (o dogfood
  FFI não cifra). Round-trips e `qpdf` continuam validando. Ver `encrypt.rs`.

## Fase 6 — Manipulação

- 🟡 **6.4 Extração de texto — ordem de leitura.** A ordem segue o fluxo do
  content stream com heurística simples de linha/espaço
  (`crates/pdf/src/extract.rs`). Layouts **multi-coluna** ou com posicionamento
  fora de ordem podem sair embaralhados. Falta: ordenação por coordenadas
  (clusterização de blocos), detecção de colunas, e melhor inferência de espaço
  por largura de glifo (hoje usa limiar fixo de `TJ < -100`).
- ✅ **6.6 Watermark/overlay (Tier 1).** Além dos bytes crus (`overlay_page`),
  `EditableDoc::watermark_text` carimba **marca-d'água de texto** diagonal
  (Helvetica padrão, opacidade via `/ExtGState /ca`+`/CA`, rotação, centralizada,
  [`WatermarkOptions`]) e `watermark_image` carimba **imagem** (XObject embutido
  no doc já existente, opacidade) em todas as páginas; merge de recursos é **deep
  merge** (não sobrescreve fontes/XObjects existentes da página). Validado por
  `qpdf --check` + `mutool` (extrai/renderiza). Falta: **carimbar outra página
  PDF como Form XObject** e marca-d'água com fonte embutida não-WinAnsi.
- ✅ **6.7 AcroForm (autoria + fill + flatten).** Autoria: `Document::text_field`/
  `checkbox`/`radio_group`/`dropdown` (`form.rs`) com `/AP` gerado e nomes
  hierárquicos. **Edição de forms existentes (Tier 1, `edit.rs`):**
  `EditableDoc::field_names` (nomes qualificados), `fill_text_field`/`set_choice`
  **geram `/AP`** (sem `NeedAppearances`), `set_checkbox`/`set_radio` ajustam
  `/V`+`/AS` pelo estado de aparência existente, e **`flatten_forms()`** pinta a
  aparência atual de cada widget no conteúdo da página (Form XObject `Do`),
  remove widgets do `/Annots` e elimina o `/AcroForm`. `qpdf --check` + `mutool`
  + `pdftotext` confirmam (valor achatado extraído). Falta: list box (só combo);
  herança/uso da fonte nomeada no `/DA` (hoje sempre Helvetica na aparência);
  XFA.
- ✅ **6.8 Otimização.** Feito: remove objetos não-referenciados + recomprime
  streams sem filtro com `FlateDecode` + renumeração compacta + **object streams
  (`/ObjStm`) e cross-reference stream (`/XRef`) na escrita** (`optimize()` ou
  `EditableDoc::compact(true)`) + **dedupe de objetos byte-idênticos** (fontes/
  recursos repetidos após merge; páginas e catálogo preservados). `qpdf --check`
  valida. Falta: recompressão de imagens / downsampling.
- ✅ **Hyperlinks (Tier 1, `lib.rs`).** `Page::link_uri` (`/Link` + ação `/URI`,
  navegação web) e `Page::link_to_page` (`/Link` + `/Dest [page /XYZ null top
  null]`, navegação interna) na geração; as anotações são resolvidas após o loop
  de páginas (refs de destino conhecidas) e mescladas no `/Annots` junto dos
  widgets de form. `qpdf --check` valida. Falta: links em texto fluido/`Report`,
  bordas/estilos visuais, destinos nomeados.
- ✅ **Bookmarks/outline (Tier 1, `outline.rs`).** `Document::add_bookmark` com
  `Bookmark` aninhável (`new`/`at_top`/`child`/`children`) → árvore `/Outlines`
  completa (`First`/`Last`/`Count`/`Next`/`Prev`/`Parent`/`Dest`) + catálogo com
  `/PageMode /UseOutlines`. `mutool show … outline` lista a árvore aninhada
  corretamente. Falta: cor/estilo (negrito/itálico) e estado aberto/fechado
  (`Count` negativo) por item.

## Rasterização de página (crate `render`, 7.8)

> Renderizar uma página → imagem (o inverso do writer). Implementado sobre
> `tiny-skia` (puro Rust, BSD-3). Cobre o grosso do conteúdo real; as lacunas
> abaixo são best-effort/adiadas.

- ✅ **Vetores** — preencher/traçar/clip (nonzero + even-odd), `cm`/`q`/`Q`,
  todos os operadores de path (`m`/`l`/`c`/`v`/`y`/`re`/`h`), dash/cap/join/
  miter, largura zero → ~1px. **Texto** com outlines reais (`ttf-parser`):
  Type0/Identity-H (caminho do próprio writer, mais sólido) + fontes simples
  (WinAnsi/Standard/MacRoman + `/Differences`, AGL subset), render modes
  (fill/stroke/invisible/clip). **Imagens** XObject + inline (amostras cruas
  1/2/4/8/16-bit por colorspace, JPEG via `jpeg-decoder`, `/SMask`,
  `/ImageMask`, `/Decode`). **Cores** Gray/RGB/CMYK/ICCBased(por `/N`)/Indexed/
  Separation/DeviceN com funções tipo 0/2/3/**4 (calculadora PostScript)**.
  **Form XObjects** (Matrix+BBox+recursão), **ExtGState** (`ca`/`CA`/`BM`),
  **sombreamentos** axial (tipo 2) e radial (tipo 3), `/Rotate` + `/CropBox`.
- 🟡 **Fallback de fonte não-embutida** usa **Roboto** (sans) para todas as
  standard-14 → métrica/forma aproximada para Times/Courier (serif/mono). Sem
  acesso a fontes do sistema. Aceitável; documentado.
- ⏳ **Mesh shadings** (tipos 4–7: free-form/lattice Gouraud, Coons, tensor) —
  não renderizados (`sh`/pattern com esses tipos não pinta). Só axial/radial.
- ⏳ **Tiling patterns** (PatternType 1) e **shading patterns** via `scn`/`SCN`
  — `sh` (axial/radial direto) funciona; preencher um path *com* um pattern
  nomeado ainda não. Hoje cai no `default_rgb` do espaço Pattern (preto).
- ⏳ **Soft mask por luminosidade/alpha do ExtGState `/SMask`** (grupos de
  transparência) — só o alfa constante `ca`/`CA` é aplicado. Blend modes
  separáveis mapeiam para os do tiny-skia; não-separáveis (Hue/Saturation/
  Color/Luminosity) caem em Normal.
- ⏳ **Codecs de imagem opacos**: `CCITTFaxDecode`, `JPXDecode` (JPEG2000),
  `JBIG2Decode` — pulados (não pintam), como na extração. Predictors de Flate
  já são tratados pelo `parser`.
- ⏳ **FontFile (Type1 PFB)** — `ttf-parser` não parseia Type1; cai no fallback
  Roboto. FontFile2 (TrueType) e FontFile3 (CFF/OpenType) funcionam.
- ⏳ **CMaps nomeados** em Type0 além de Identity-H/V (ex.: cmaps CJK
  predefinidos) — tratados como identidade 2-byte (best-effort).
- Nota: a saída raster é **determinística** (tiny-skia é determinístico, sem
  timestamps), mas o invariante de "PDF byte-idêntico" não se aplica aqui —
  `render` produz imagens, não PDFs, e não toca o grafo de objetos `Send`.

## Writer / núcleo

- ✅ **Object streams e cross-reference streams na escrita** (`crates/writer`).
  `Document::set_object_streams(true)` (exposto via `EditableDoc::compact`/
  `optimize`) empacota todo objeto não-stream num `/ObjStm` e emite um `/XRef`
  stream. Desativado automaticamente quando há criptografia (mantém a cifragem
  por-objeto correta). O *parser* já lia ambos; agora há round-trip completo.
- ✅ **Incremental update genérico** — `EditableDoc::to_bytes_incremental(orig)`
  / `save_incremental` anexa só os objetos alterados/novos + uma nova seção xref
  clássica encadeada por `/Prev`, preservando os bytes originais verbatim (não
  invalida assinaturas existentes). Diff por bytes serializados; objetos deletados
  viram entradas livres. `qpdf --check` valida. (`sign.rs` mantém seu próprio
  caminho especializado para assinaturas.)

## FFI + binding Python

- ✅ **JPEG CMYK no rasterizador (FINDING-007, 2026-07)**: `jpeg-decoder` já
  devolve CMYK verdadeiro (resolve APP14/YCCK sozinho); o `render` invertia de
  novo → K=0 virava K=1 → retângulo preto. Agora a inversão só acontece quando
  o `/Decode` inverte **e** o JPEG não é Adobe (o `/Decode [1 0…]` do embed
  compensa o armazenamento invertido que o decoder já desfez — validado contra
  mutool, que renderiza o mesmo arquivo ciano). Embed já estava correto
  (`/DeviceCMYK` + `/Decode` com APP14). Fixture `cmyk_adobe.jpg` committado.
  Pendente: conversão via perfil ICC embutido no JPEG (hoje naive CMYK→RGB;
  mutool usa ICC — diferença perceptual pequena, mesmo matiz).

- ✅ **Redação glyph-level (FINDING-006, 2026-07)**: `EditableDoc::redact` agora
  remove de verdade — interseção por **glifo** com métricas das fontes da página
  (substituição por deslocamentos `TJ`, vizinhos mantêm posição), `Do` derrubado
  por overlap de bbox (recurso podado + objeto anulado quando sem referência),
  anotações interceptantes removidas, e **falha ruidosa** (`RedactError` /
  `PdfStatus::Unsupported=13`) para inline images/stream não-decodável — nunca
  pinta caixa sobre dado ainda presente. Pendências: mascaramento pixel-a-pixel
  de imagem parcialmente coberta (hoje remove a imagem inteira — conservador) e
  redação dentro de Form XObjects mantidos (o Form que intersecta é derrubado
  por inteiro).

- ⏳ **Estampagem com fonte embutida + âncora vertical + parágrafo com quebra
  só em core+FFI+C#** (FINDING-001/002/003, 2026-07): `EditableDoc::add_font`/
  `add_font_file`, `place_text_with_font*`, `masked_text_with_font*`, os
  controles de âncora vertical `VerticalAnchor` (Baseline/Top/Bottom em
  `place_text_anchored`/`pdf_editable_place_text_anchored`) e `VerticalAlign`
  (Top/Middle/Bottom em `masked_text_valign`/`pdf_editable_masked_text_valign`),
  a estampagem de **parágrafo com word-wrap** `place_paragraph`/
  `place_paragraph_with_font` (`pdf_editable_place_paragraph`; âncora topo,
  align 4 modos, `max_height`, `line_height`), e o **modo de espaço de
  coordenadas** `StampSpace` Visible/Media (`set_stamp_space`/
  `pdf_editable_set_stamp_space`; Media = espaço bruto da mídia estilo iText,
  sem compor o `/Rotate` da página — FINDING-004), as **âncoras de line-box
  iText** `VerticalAnchor::LineTop`/`LineBottom` (métricas OS/2 win — ou
  typo×1.2 — + meio-leading 0.21em, calibradas contra iText `SetFixedPosition`),
  as **âncoras de bloco** do parágrafo (`place_paragraph_anchored`/
  `_with_font_anchored` + `pdf_editable_place_paragraph_anchored`; Bottom =
  **bottom-pin**: o fundo do bloco repousa em `y`, `max_height` é teto que
  corta pelo topo) e o **inset configurável** do masked text
  (`masked_text_padded`/`_with_font_padded` + `pdf_editable_masked_text_pad`;
  pad 0 = rente à borda, estilo Syncfusion) existem no core, na FFI e no
  binding C# (params opcionais `fontId`/`anchor`/`valign`/`padding`/
  `rotationDeg`; `PlaceParagraph`/`PlaceParagraphCounted`/
  `PlaceParagraphMeasured` com altura consumida; propriedade
  `EditableDoc.StampSpace`). Âncoras `Line*` do parágrafo usam o **advance
  iText** (métricas selecionadas + 0.35em) como leading; rotação de parágrafo
  gira o bloco em torno da âncora (pivô documentado e testado); `DrawImage`
  ganhou `ImageAnchor` Corner/BoundingBox (`draw_image_anchored` /
  `pdf_editable_draw_image_anchored`; BoundingBox = bbox rotacionado com canto
  em (x,y), estilo iText). Contrato de coordenadas em `docs/COORDINATES.md`.
  O crate `fonts` expõe as métricas OS/2 cruas (`typo_*`/`win_*`). Os outros
  **9 bindings** (Python, Node, Go, PHP, Ruby, Java, Swift, Delphi, Rust)
  ainda não expõem essas APIs — portar quando o loop de validação C#
  estabilizar.

- ✅ **Superfície do C ABI completa** (~80 exports, `crates/ffi/src/{lib,build,
  editable,signing,verify}.rs`): gráficos vetoriais, **fontes+texto+parágrafos**,
  **imagens+figura**, **PDF/A 1b–3a**, **tagging/heading**, **anexos**, **forms**
  (texto/checkbox/radio/dropdown), `PdfEditable` (load/merge/split/rotate/reorder/
  delete/info/xmp/overlay/fill/optimize/compact/incremental/encrypt/save),
  **extract_text**, e **assinatura** (`pdf_sign`/`pdf_timestamp`/`pdf_add_dss`).
  **Tier 1/2 expostos (2026-06):** hyperlinks (`pdf_page_link_uri`/`link_to_page`),
  bookmarks (`pdf_document_add_bookmarks`, lista plana com `levels`), ZUGFeRD/
  Factur-X (`pdf_document_facturx`), fill+flatten de forms existentes
  (`pdf_editable_set_checkbox`/`set_radio`/`set_choice`/`flatten_forms`/
  `field_names`), watermark (`pdf_editable_watermark_text`/`watermark_image_file`),
  redação (`pdf_editable_redact`), conversão PDF→PDF/A
  (`pdf_editable_convert_to_pdfa`) e validação de assinatura
  (`pdf_verify_signatures_json`, retorna JSON). Header `include/pdf.h` regenerado
  por cbindgen. **As 14 novas funções estão em TODOS os 10 bindings** (Python,
  C#, Go, PHP, Ruby, Node, Java, Delphi, Swift, Rust), cada um com seu smoke test
  estendido e passando.
- ✅ **Binding Python completo** (`bindings/python/rustpdf`): wrappers idiomáticos
  `Document`/`EditableDoc` + funções `extract_text`/`sign`/`timestamp`/`add_dss`,
  enums (`PdfaLevel`/`Align`/`AFRelationship`/`Encryption`), empacotamento
  `pyproject.toml`. `test_binding.py` mantém a **byte-identidade** (dogfood) e
  exercita toda a superfície.
- ✅ **Binding C#/.NET completo** (`bindings/csharp/RustPdf`): P/Invoke gerado por
  fonte (`LibraryImport`), wrappers `Document`/`EditableDoc`/`Pdf` + enums, resolver
  de lib nativa, projeto `net8.0`; `bindings/csharp/Sample` (`make csharp-test`).
- ✅ **Binding Go completo** (`bindings/go/rustpdf`, cgo): `Document`/`EditableDoc`
  + funcs de pacote + enums + tipo `Error`; `go test` exercita toda a superfície
  (`make go-test`, agora via tag `rustpdf_dev`). **Pronto para publicar:** module
  path corrigido (`github.com/rustpdf/rustpdf/bindings/go`), `pdf.h` vendorizado
  ao lado das fontes, e linkagem por build tag — `link_dev.go` (`rustpdf_dev`,
  dylib do build tree) vs `link_dist.go` (default, `.a` estático por plataforma em
  `rustpdf/lib/<os>_<arch>/`). `make go-dist` (`bindings/go/scripts/package.sh`)
  builda os `.a` por target (best effort, igual swift-dist). Publicação = git tag
  **prefixada** `bindings/go/vX.Y.Z` com os `.a` force-adicionados (mantidos fora
  da branch por `lib/.gitignore`); CI deve buildar com o `RUSTPDF_LICENSE_PUBKEY`
  de produção. Falta: rodar go-dist nas 5 plataformas em CI + push da tag.
- ✅ **Binding PHP completo** (`bindings/php`, `ext-ffi`): `RustPdf\{Pdf,Document,
  EditableDoc}` + enums (PSR-4); `php bindings/php/test/run.php` (`make php-test`).
- ✅ **Binding Ruby completo** (`bindings/ruby`, Fiddle stdlib): `RustPdf::
  {Document,EditableDoc}` + módulo de funções + enums; `make ruby-test`.
- ✅ **Binding Node.js/TypeScript completo** (`bindings/node`, Koffi FFI puro):
  `RustPdf.{Document,EditableDoc}` + funcs + enums + tipos `index.d.ts`; `node
  bindings/node/test/run.js` (`make node-test`). **Empacotado para npm** (não
  publicado): layout de `optionalDependencies` por plataforma (estilo
  esbuild) — pacote principal `rustpdf` + 4 pacotes `@rustpdf/<plataforma>`
  (`darwin-arm64`, `linux-x64-gnu`, `linux-arm64-gnu`, `win32-x64-msvc`) com a
  cdylib e seletores `os`/`cpu`/`libc`; loader resolve via `require.resolve` do
  pacote-plataforma com fallback `target/`; `scripts/sync-versions.mjs` mantém
  versões em sincronia; CI `.github/workflows/release-node.yml` (tag `node-v*`).
  **Publicado: `npm install rustpdf` (0.1.0)** — `rustpdf` +
  `@rustpdf/{darwin-arm64,linux-x64-gnu,linux-arm64-gnu,win32-x64-msvc}` no ar
  (release pela tag `node-v0.1.0`; principal é JS puro, publicado direto).
- ✅ **Binding Java/JVM completo** (`bindings/java`, JNA FFI puro): `dev.rustpdf.
  {Pdf,Document,EditableDoc}` (AutoCloseable) + enums; `make java-test`.
- ✅ **Binding Delphi/Free Pascal completo** (`bindings/delphi/RustPdf.pas`, FFI
  puro com carregamento dinâmico da cdylib): `TPdfDocument`/`TPdfEditable` +
  record `Pdf` + enums; compila em Delphi 10.x+ e FPC 3.2+; `make delphi-test`
  (`bindings/delphi/test/run.dpr`, pula sem `fpc`/`dcc64`).
  Distribuição Delphi: `make delphi-dist` (`scripts/package.sh`) gera um zip
  versionado (unit + `lib/<os-arch>/` por target Rust instalado + sample +
  `boss.json`); o resolver acha a cdylib ao lado do executável. Falta: rodar o
  empacotamento no CI com os targets de cross-compile (win-x64/x86, linux-x64,
  macos universal) e assinar/notarizar a dylib do macOS.
- ✅ **Binding Swift completo** (`bindings/swift`, SwiftPM, ligação por target C
  `CRustPdf` sobre cópia do `pdf.h`): `Document`/`EditableDoc` (reference types,
  handle liberado em `deinit`) + enum `Pdf` + enums Swift; `make swift-test` roda
  o `SmokeTest` (`swift test`, liga `target/debug`; pula sem `swift`), e `swift
  run rustpdf-example` é um demo.
  Distribuição Swift: `make swift-dist` (`bindings/swift/scripts/package.sh`)
  monta `RustPdfFFI.xcframework` **estático** (macOS universal + iOS device + iOS
  simulator) e um pacote consumível em `bindings/swift/dist/` (`.binaryTarget`,
  liga `iconv`), mais o `.xcframework.zip` + checksum para `.binaryTarget(url:)`.
  Estático → funciona dentro de app bundle iOS sem sidecar. Falta: rodar o
  empacotamento no CI, hospedar o zip em `/downloads/`, e (opcional) slices
  tvOS/visionOS/Catalyst (basta `rustup target add` antes do `swift-dist`).
  Refino: bindings de outras linguagens (Dart/Flutter), empacotamento com a lib
  nativa embutida (NuGet/wheel/Packagist/gem/npm/Maven com binários por
  plataforma), **WASM** para edge/serverless, e `/Span` inline + assinatura
  visível ainda não expostos na borda C.

## Licenciamento (corporativo)

- ✅ **Implementado** (`crates/license`, Ed25519 via `ed25519-dalek`). Token
  offline assinado (`hex(payload).hex(sig)`) com `expires`; `pdf::activate_license`
  + `pdf::require(Feature)` bloqueiam **PDF/A, assinatura/PAdES, criptografia,
  acessibilidade** sem licença válida (→ `BuildError::License`/`SignError::License`/
  `PdfStatus::License`/Python `PdfError`). Chave pública embutida (override de
  build `RUSTPDF_LICENSE_PUBKEY`); emissor `licctl`; testes em `licensing.rs`.
  Refinos futuros: revogação online/CRL de licenças, binding por máquina
  (node-locking), e ofuscação anti-tamper do binário (defesa em profundidade —
  a segurança criptográfica de emissão já está garantida). Ver `docs/LICENSING.md`.

## Tooling

- ✅ **`verapdf` instalado** (brew, v1.30.2, Java 22) — usado para validar PDF/A
  (7.4 confirmado). `testkit::validate()` roda só validadores estruturais
  (qpdf/mutool); use `validate_with(&[Validator::VeraPdf])` para PDF/A. Servirá
  também para PDF/UA (7.5).
- ℹ️ **Sandbox de teste não consegue spawnar `qpdf`** — testes que precisam de
  qpdf usam fixtures commitadas e re-parse próprio; o `qpdf --check` real é
  rodado manualmente no shell. (Ver `CLAUDE.md`.)

## Fase 7 — Diferenciadores (Tier 3)

Feito: **7.1** (assinatura digital), **7.3** (cripto na escrita RC4/AES-128 +
permissões) e **7.6** (engine de layout). Pendentes/parciais:

- 🟡 **7.1 Assinatura digital** — núcleo + extras feitos (`pdf::sign`):
  incremental update + `ByteRange` + PKCS#7/CMS detached SHA-256/RSA;
  **assinatura visível** (appearance stream Helvetica, sem embed);
  **múltiplas assinaturas** (cada uma um novo update incremental; anteriores
  continuam válidas); **cadeia de certificados** no CMS. `pdfsig` valida todas.
  **Validação de assinatura (Tier 2, `verify.rs`):** `pdf::verify_signatures`
  localiza cada dict de assinatura, recomputa o digest do `/ByteRange`, faz parse
  do CMS e verifica **(a)** a assinatura RSA PKCS#1 v1.5 sobre os signedAttrs com
  a chave pública do certificado, **(b)** o atributo `messageDigest` == digest dos
  bytes cobertos, e **(c)** se cobre o documento inteiro (`SignatureReport`). Casa
  com `pdfsig` ("Signature is Valid") e detecta adulteração. **Licenciada
  (Enterprise):** gated por `Feature::Signatures`; retorna `BuildError` (mapeado
  para `PdfStatus::License` no FFI). Pendente: cadeia de confiança/revogação
  (precisa de infra), ECDSA/RSA-PSS.
  **Ainda pendentes:** validação no **Acrobat** (sem acesso); timestamp/LTV
  (ver 7.2); e o `/M` (data) é fixo por padrão.
- 🟡 **7.2 PAdES + LTV.** **Feito (offline):** B-B (`ETSI.CAdES.detached` +
  `signing-certificate-v2`), B-LT (`add_dss` → `/DSS` com `/Certs`/`/CRLs`) e
  B-LTA (`timestamp` → `/DocTimeStamp` `ETSI.RFC3161`, token RFC 3161 real
  verificado com `openssl cms -verify`). **Pendentes (precisam de infra externa
  ou são refinamentos):**
  - **TSA confiável via rede** — hoje o TSA é auto-emitido (offline). Falta um
    cliente HTTP RFC 3161 para um TSA público/qualificado.
  - **Busca automática de OCSP/CRL** — hoje o chamador passa os DER; falta buscar
    do AIA/CDP (rede).
  - **`signature-time-stamp`** (timestamp do *signatário*, atributo não-assinado
    no SignerInfo) — além do document timestamp já feito.
  - **`/VRI`** (Validation-Related Info por assinatura) e **`/OCSPs`** no DSS.
  - `issuerSerial` no ESSCertIDv2 (hoje só `certHash`); ECDSA/RSA-PSS/SHA-384-512.
  - Validação num verificador PAdES de referência (DSS/Adobe) — sem acesso aqui.
- ✅ **Issue #41 P0 — assinatura diferida / HSM ("bring your own signer").** A
  chave privada **nunca** entra na biblioteca:
  - **Model A — callback externo** (`pdf::sign_with`): a lib monta os
    signedAttrs do CMS e chama de volta para a assinatura RSA bruta (do HSM —
    Azure Key Vault/VIDaaS/BirdID), depois assembla e embute o CMS. Prova de
    correção: `sign_with` delegando à mesma chave gera **bytes idênticos** ao
    `sign` local (teste `external_signing_matches_local_signing_byte_for_byte`).
  - **Model B — duas fases** (`pdf::begin_signing` → `SigningSession::complete`
    ou o stateless `pdf::complete_signing`): a fase 1 devolve o PDF preparado + o
    hash/bytes a assinar; o integrador (BouncyCastle/SignedCms nos servidores
    deles) constrói o container CMS e a fase 2 o injeta no placeholder. Cruza
    fronteira async/HTTP.
  - **Tamanho reservado configurável** (`SignOptions::estimated_size`) para
    containers de HSM que variam.
  - **DocMDP / certificação** (`Certify`, `/Perms /DocMDP` +
    `/Reference` no dict de assinatura) — P=1/2/3 (`CERTIFIED_FORM_FILLING_AND_ANNOTATIONS`).
  - **Signature policy identifier** (`SignaturePolicy`, atributo assinado
    `id-aa-ets-sigPolicyId`, PAdES-EPES) para a Política de Assinatura ICP-Brasil
    (AD-RB) com OID + hash + SPURI.
  - **Listar assinaturas existentes** antes de assinar (`pdf::list_signatures`,
    equivalente ao `SignatureUtil.getSignatureNames` do iText).
  - **FFI** (`pdf_sign_begin`/`pdf_sign_complete`/`pdf_sign_with` +
    `PdfSigningOptions`/`PdfSignHashFn`, `pdf_list_signatures`) + **binding
    C#** (`Pdf.SignWith`/`BeginSigning`/`CompleteSignature`/`ListSignatures`,
    `IRemoteSigner`, `SigningOptions`, `SignaturePolicy`) — exercitados no `Sample`.
  - ✅ **Validado com cert ICP-Brasil REAL (2026-06-29):** assinado um PDF com
    um e-CNPJ A1 (RFB/AC Certisign RFB G5) que reside **só no Keychain do macOS**,
    via `sign_with` (chave nunca exportada — helper Swift `SecKeyCreateSignature`);
    cadeia completa embutida. **APROVADO no Verificador de Conformidade do ICP-Brasil
    (ITI)** + `pdfsig` "Signature is Valid" + `openssl cms -verify` "successful".
    Exemplos `crates/pdf/examples/sign_icp_keychain.rs` + `verify_pdf.rs`.
  - ✅ **Bug corrigido (exposto pelo teste real):** `verify.rs::signer_cert`
    pegava o **primeiro** cert do CMS SET; com cadeia embutida o `cms` crate
    ordena o SET por DER e o signatário não fica em 1º → `signature_valid` dava
    falso-negativo. Agora casa o cert pelo `SignerIdentifier` (issuer+serial).
    Regressão `signature_with_embedded_chain_verifies`. Corrige o `.ValidateSignature()`
    (P1 #5) em **todos** os bindings.
  - ✅ **APROVADO no Verificador de Conformidade do ICP-Brasil (ITI/VALIDAR)** com
    e-CNPJ real: PAdES AD-RB (DocMDP P=2 `Certify::Forms` + política PAdES OID
    `2.16.76.1.7.1.11.1.3` / `PA_PAdES_AD_RB_v1_3.der` + cadeia até a Raiz v5).
    Descoberta-chave: a política **PAdES** tem arco de OID próprio
    (`2.16.76.1.7.1.11.x`), distinto do **CAdES** (`2.16.76.1.7.1.1.x`); usar o do
    CAdES num PAdES reprova. Exemplo `crates/pdf/examples/sign_icp_keychain.rs`.
  - ✅ **Deferred signing propagado para TODOS os bindings** (Python/Node/Go/Ruby/
    PHP/Java/Swift/Delphi + C#): `SignWith`/`BeginSigning`/`CompleteSignature`/
    `ListSignatures` + `SigningOptions`/`SignaturePolicy`/`Certify`, cada um com o
    mecanismo de callback FFI nativo da linguagem (Model A provado end-to-end no
    smoke test de cada binding). Página `/icp-brasil` no site (EN).
  - ✅ **Issue #41 P1 — concluído (2026-06-29).** Em core + C ABI + **todos os 10
    bindings** (smoke tests verdes):
    - **#6 Busca posicional de texto** → `pdf::find_text(bytes, query, FindOptions)`
      devolve `Vec<TextHit>` (page, text, x/y/w/h em pontos no espaço do usuário).
      Novo `crates/pdf/src/find.rs` interpreta o content stream (CTM/Tm + avanços
      de `/Widths`/`/W`+`/DW`) e une as caixas dos glifos casados. FFI
      `pdf_find_text_json` (JSON). **Livre** (como a extração).
    - **#5 Inspeção rica de assinatura** — `SignatureReport` agora tem `issuer`,
      `serial_number` (hex), `valid_from`/`valid_to` (ISO-8601), `algorithm`
      (ex. SHA256withRSA), `signing_time`, `cert_count`, `has_timestamp`.
      JSON do FFI `pdf_verify_signatures_json` estendido.
    - **#4 Imagem na assinatura visível** — `VisibleSignature.image` (PNG/JPEG)
      desenhada aspect-fit atrás do texto; novos campos `vis_*` em `PdfSigningOptions`.
    - **#7 Desenhar sobre PDF** — geometria de página honra `/CropBox` + herança
      do page-tree e troca dims em `/Rotate` 90/270 (`EditableDoc::page_dimensions`);
      watermarks saem "de pé" em páginas rotacionadas; `watermark_image` ganhou
      rotação; `WatermarkOptions.opaque_background` (carimbo branco opaco).
    - **#8 Normalização** — `EditableDoc::set_version`/`strip_pdfa`/`normalize`
      (downgrade de versão + remoção de OutputIntents/XMP-pdfaid/Version). FFI
      correspondente.
    - **AD-RT (TSA em rede)** — helpers transport-agnósticos (mantêm o core
      offline): `begin_timestamp` + `timestamp_request` (DER do RFC 3161
      TimeStampReq) + `timestamp_token_from_response`; o integrador faz o POST e
      embute via o `complete_signing` existente. FFI `pdf_timestamp_begin`/
      `pdf_timestamp_request`/`pdf_timestamp_token_from_response`. Página
      `/search-pdf` no site.
  - **Ainda pendentes do issue #41:** **Adobe Acrobat** (trusted, sem warnings —
    requer as raízes ICP-Brasil/AATL no validador); **cliente OCSP/CRL de rede
    automático** (o `add_dss` aceita DER fornecido pelo integrador); P2 (layout
    multi-coluna) aberto.
  - ✅ **Issue #45 P1 — concluído (2026-06-30).** Em core + C ABI + **todos os 10
    bindings** (smoke tests verdes). Nomes escolhidos para serem amigáveis ao dev
    e **não imitar o iText** (esquema descritivo):
    - **#1 Geometria de página (read-only)** → `pdf::measure_pages(bytes)` /
      `measure_page(bytes, i)` devolvem `PageGeometry { page, width, height,
      rotation, rotated_width, rotated_height, media_box, crop_box: PdfRect }`
      (pontos; `rotated_*` troca dims em 90/270; herda MediaBox/CropBox/Rotate do
      page-tree e intersecta crop∩media). Novo `crates/pdf/src/geometry.rs`. FFI
      `pdf_measure_pages_json` (JSON). **Livre.**
    - **#2 Retângulo preenchido + texto posicionado/rotacionado** →
      `EditableDoc::fill_rect(page, x, y, w, h, (r,g,b), opacity)` e
      `place_text(page, x, y, text, size, (r,g,b), rotation_deg)` carimbam sobre
      páginas existentes (coords no espaço **visível**, honrando `/Rotate`; nomes
      de recurso únicos por gstate). FFI `pdf_editable_fill_rect`/
      `pdf_editable_place_text` (out_found). **Livre.**
    - **#3 Inspeção não-mutante** → `pdf::inspect(bytes)` devolve `PdfOverview {
      version, pdfa_level, encrypted, encryption, requires_password, page_count }`.
      Novo `parser::probe_encryption`/`header_version` (lê `/Encrypt` sem
      decifrar o corpo, então funciona em arquivos protegidos por senha) +
      `crates/pdf/src/inspect.rs` (nível PDF/A do XMP `pdfaid`). FFI
      `pdf_inspect_json`. **Livre.** Cross-validado contra fixtures do qpdf
      (RC4 40/128, AES-128, AES-256/R6).
    - **P2:** **macOS x64 (osx-x64)** adicionado ao release C# (cross-compile no
      runner arm64, smoke pulada nesse RID); **overload async de assinatura
      remota** `Pdf.SignWithAsync(Func<byte[],Task<byte[]>>)` no C#; **layout
      de alto nível já existe** (`pdf::Report`/`Table`, `flow.rs`) — confirmado.
    - Bindings: nomes idiomáticos por linguagem (snake_case Python/Ruby/Rust,
      PascalCase Go/C#/Java/Delphi/Swift, camelCase Node/PHP). Ruby usa
      `inspect_pdf` (não clobberar `Object#inspect`).
- ✅ **7.4 PDF/A — níveis 1b/2b/2a/3b/3a.** `Document::pdfa()` (=A-2b),
  `pdfa_a()` (=A-2a) e `pdfa_with(PdfaLevel)` cobrem **A-1b** (header PDF 1.4,
  `/CIDSet` no descritor lido do programa de subset, sem object streams),
  **A-2b/2a**, e **A-3b/3a** (anexos via `attach_file` → `/EmbeddedFile` +
  `/AFRelationship` + `/AF` + `/Names /EmbeddedFiles`). **veraPDF valida 1b, 2b,
  2a, 3b e 3a** (`isCompliant=true`). **Conversão de PDF lido → PDF/A (Tier 2):**
  `EditableDoc::convert_to_pdfa(PdfaLevel)` adiciona OutputIntent sRGB + XMP
  `pdfaid` (sincronizado com `/Info`) + `/ID`, força 1.4 em A-1b, e **falha com
  `ConvertError::FontsNotEmbedded`** se alguma fonte não estiver embutida (PDF/A
  exige todas embutidas) ou `TaggingRequired` para nível A. **veraPDF valida 2b**
  no doc convertido (round-trip). Pendente: níveis A (precisa de tags), flatten de
  transparência para A-1, e embutir fontes faltantes (precisa das fontes).
- ✅ **Redação real (Tier 2, `redact.rs`).** `EditableDoc::redact(page, rects)`
  interpreta o content stream (pilha CTM via `q`/`Q`/`cm`, matrizes de texto
  `Tm`/`Td`/`T*`) e **remove** os operadores de texto/imagem cuja origem cai
  dentro de um retângulo — o conteúdo some do arquivo (não é extraível), não é só
  tapado — e pinta retângulos pretos opacos por cima. Decodifica content
  uncompressed e `FlateDecode`; faz bail (só tapa) com imagens inline. Testes
  confirmam que o texto redigido some de `extract_text`. **Licenciada (Enterprise):**
  novo `Feature::Redaction` (bit 4), checado no `to_bytes`/`to_bytes_incremental`
  via flag `redacted`; tokens Enterprise/OEM incluem; o token dev de teste foi
  re-emitido (`licctl ... all`). Pendente: redação parcial dentro de um run
  (granularidade hoje = operador de show), imagens inline, e remoção em `/Annots`.
- ✅ **ZUGFeRD / Factur-X (Tier 2).** `Document::facturx(xml, FacturxProfile)`
  marca PDF/A-3b, embute `factur-x.xml` (`/AFRelationship /Alternative`) e injeta
  a descrição `fx` + **schema de extensão pdfaExtension** no XMP. **veraPDF valida
  PDF/A-3b**. Pendente: validar o XML contra o esquema CII (semântica da fatura,
  fora de escopo da lib) e o perfil ZUGFeRD 1.0 legado (`zf`).
- 🟡 **7.5 Tagged PDF / acessibilidade.** **Feito** (`Document::tagged()` /
  `pdfa_a()`): structure tree **aninhado**, marked content, ParentTree, MarkInfo,
  Lang, ViewerPreferences, XMP `pdfaid`+`pdfuaid`. **Tags semânticas feitas:**
  `/H1`–`/H6` (`TextObject::tag`/`Paragraph::tag`/`Report::heading_level`),
  `/Figure` + `/Alt` (texto alternativo UTF-16BE via `Page::figure`), e
  `/Table`/`/TR`/`/TH`/`/TD` com `/Scope` **e** associação `/Headers`↔`/ID` nos
  cabeçalhos, **listas `/L`/`/LI`/`/LBody`** (`Report::list`) e **`/Caption`**
  (via `tag()`). `Report` gera tabelas e listas marcadas automaticamente.
  marca-d'água/tabelas, e **`/Span` inline por-run** (`TextObject::show_span` →
  marked content aninhado `/P …BDC … /Span<</MCID>>BDC…EMC … EMC`, com o elemento
  `Span` filho do `P` via `/K` misto). **`verapdf` valida PDF/A-2a e PDF/UA-1** no
  doc básico, rico (heading+figura+tabela), fino (lista+caption+headers) e com span
  inline. `StructTag` é público para uso manual. Refino restante: validação no PAC
  humano e listas/spans dentro de células de tabela.
- ⏳ **7.7 HTML/CSS → PDF** — efetivamente um motor de layout de browser; o
  próprio `project.md` recomenda tratar como **produto à parte**. Fora de escopo.

## Fase 7.9 — PDF 2.0 + PDF/A-4 (ISO 32000-2 / ISO 19005-4)

✅ **Feito (2026-06-28).** `PdfVersion::V2_0` (header `%PDF-2.0` + catálogo
`/Version /2.0`); **PDF/A-4 / A-4e / A-4f** (`Document::pdfa4()`/`pdfa4f()`/
`pdfa4e()`, `PdfaLevel::{A4,A4e,A4f}`). XMP usa `pdfaid:part=4`+`pdfaid:rev=2020`
(sem `conformance` no nível base; `E`/`F` para as variantes); `/CIDSet` e `/Info`
omitidos (deprecados no PDF 2.0; `/Info` sem `/PieceInfo` é proibido — pego pelo
veraPDF). **veraPDF confirma `isCompliant=true` para A-4 e A-4f.** FFI: versão
int `3`=2.0, pdfa level `5`/`6`/`7`.

✅ **Todas as 10 bindings atualizadas** (Python/C#/Go/PHP/Ruby/Node/Java/Delphi/
Swift/Rust): constantes `A4`/`A4e`/`A4f` (=5/6/7) em cada `PdfaLevel`, e `V2_0`
(=3) onde há enum de versão tipado (Rust `PdfVersion::V2_0`, Swift `PdfVersion.v20`);
as demais recebem versão como `int` direto (doc-comments → `3=2.0`). Sem mudança no
`pdf.h`. Validado: **Python** gera PDF/A-4 (`%PDF-2.0` + pdfaid part4/rev2020) e
PDF/A-4f (conformance F) ponta-a-ponta; Rust binding compila; Go `gofmt` limpo;
PHP/Ruby/Node sintaxe OK. ⏳ Falta rodar os smokes completos por linguagem
(`make {csharp,go,…}-test`) — exigem build do cdylib + toolchain de cada uma.

- ⏳ **PDF/A-4 a partir de leitura** (`EditableDoc::convert_to_pdfa(A4)`): o caminho
  já seta versão 2.0 + XMP, mas falta validar com veraPDF um PDF lido→A-4
  (remover `/Info`/CIDSet do doc existente como no caminho de autoria).

## Próximas fases

Nenhuma fase nova além da 7. Concluídos em 2026-06-25: AES-256 (R/W), object
streams na escrita, tags semânticas (H/Figure/Table), **PDF/A-1b/2b/2a/3b/3a**,
**AcroForm (texto/checkbox/radio/choice + `/AP` + nomes hierárquicos)**, **dedupe
de objetos**, **incremental update genérico**, **listas/caption/Headers-IDs**,
**`/Span` inline por-run** e **hardening de cripto com CSPRNG**. Refinos restantes:
edição de forms existentes com `/AP`, conversão de PDF lido → PDF/A, e itens que
dependem de rede (TSA/OCSP).
