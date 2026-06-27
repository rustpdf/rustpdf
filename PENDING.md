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
- 🟡 **6.6 Watermark/overlay.** Hoje aceita bytes de content stream crus sobre a
  página (`overlay_page`). Faltam conveniências: **texto de marca-d'água**
  (precisa embutir fonte no doc já existente — a pipeline de fontes não está
  conectada ao `EditableDoc`) e **carimbar outra página PDF como Form XObject**.
- ✅ **6.7 AcroForm (autoria).** `Document::text_field`/`checkbox`/`radio_group`/
  `dropdown` (`form.rs`) criam um `/AcroForm` completo com **appearance streams
  `/AP` gerados** (Helvetica/ZapfDingbats no `/DR`, sem `NeedAppearances`),
  **checkbox/radio/choice** além de texto, e **nomes hierárquicos** (`a.b.c` →
  campos-pai aninhados com `/Kids`+`/Parent`). `qpdf` lê todos os campos com
  fullname/valor corretos. Pendente: edição de forms **existentes** com geração
  de `/AP` (hoje `EditableDoc::fill_text_field` ainda usa `NeedAppearances`); list
  box (só combo); herança de `/DA` por campo.
- ✅ **6.8 Otimização.** Feito: remove objetos não-referenciados + recomprime
  streams sem filtro com `FlateDecode` + renumeração compacta + **object streams
  (`/ObjStm`) e cross-reference stream (`/XRef`) na escrita** (`optimize()` ou
  `EditableDoc::compact(true)`) + **dedupe de objetos byte-idênticos** (fontes/
  recursos repetidos após merge; páginas e catálogo preservados). `qpdf --check`
  valida. Falta: recompressão de imagens / downsampling.

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

- ✅ **Superfície do C ABI completa** (~60 exports, `crates/ffi/src/{lib,build,
  editable,signing}.rs`): gráficos vetoriais, **fontes+texto+parágrafos**,
  **imagens+figura**, **PDF/A 1b–3a**, **tagging/heading**, **anexos**, **forms**
  (texto/checkbox/radio/dropdown), `PdfEditable` (load/merge/split/rotate/reorder/
  delete/info/xmp/overlay/fill/optimize/compact/incremental/encrypt/save),
  **extract_text**, e **assinatura** (`pdf_sign`/`pdf_timestamp`/`pdf_add_dss`).
  Header `include/pdf.h` regenerado por cbindgen.
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
  (`make go-test`).
- ✅ **Binding PHP completo** (`bindings/php`, `ext-ffi`): `RustPdf\{Pdf,Document,
  EditableDoc}` + enums (PSR-4); `php bindings/php/test/run.php` (`make php-test`).
- ✅ **Binding Ruby completo** (`bindings/ruby`, Fiddle stdlib): `RustPdf::
  {Document,EditableDoc}` + módulo de funções + enums; `make ruby-test`.
- ✅ **Binding Node.js/TypeScript completo** (`bindings/node`, Koffi FFI puro):
  `RustPdf.{Document,EditableDoc}` + funcs + enums + tipos `index.d.ts`; `node
  bindings/node/test/run.js` (`make node-test`).
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
- ✅ **Binding Swift completo** (`bindings/swift`, SwiftPM, FFI puro com
  `dlopen`/`dlsym`): `Document`/`EditableDoc` (reference types, handle liberado
  em `deinit`) + enum `Pdf` + enums Swift; resolver acha a cdylib via
  `RUSTPDF_LIB` → ao lado do executável → `target/{debug,release}`; `make
  swift-test` roda o `SmokeTest` (`swift test`, pula sem `swift`), e `swift run
  rustpdf-example` é um demo. Falta: empacotar como binary xcframework/artefato
  SwiftPM com a cdylib embutida por plataforma e publicar.
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
- ✅ **7.4 PDF/A — níveis 1b/2b/2a/3b/3a.** `Document::pdfa()` (=A-2b),
  `pdfa_a()` (=A-2a) e `pdfa_with(PdfaLevel)` cobrem **A-1b** (header PDF 1.4,
  `/CIDSet` no descritor lido do programa de subset, sem object streams),
  **A-2b/2a**, e **A-3b/3a** (anexos via `attach_file` → `/EmbeddedFile` +
  `/AFRelationship` + `/AF` + `/Names /EmbeddedFiles`). **veraPDF valida 1b, 2b,
  2a, 3b e 3a** (`isCompliant=true`). Pendente: converter um PDF *lido* para
  PDF/A (hoje só na geração); A-1 com imagens transparentes exigiria flatten.
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

## Próximas fases

Nenhuma fase nova além da 7. Concluídos em 2026-06-25: AES-256 (R/W), object
streams na escrita, tags semânticas (H/Figure/Table), **PDF/A-1b/2b/2a/3b/3a**,
**AcroForm (texto/checkbox/radio/choice + `/AP` + nomes hierárquicos)**, **dedupe
de objetos**, **incremental update genérico**, **listas/caption/Headers-IDs**,
**`/Span` inline por-run** e **hardening de cripto com CSPRNG**. Refinos restantes:
edição de forms existentes com `/AP`, conversão de PDF lido → PDF/A, e itens que
dependem de rede (TSA/OCSP).
