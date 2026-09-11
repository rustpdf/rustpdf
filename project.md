# Escopo — Biblioteca PDF em Rust (open source MIT, com porte multi-linguagem)

> Documento de planejamento. Cada tarefa tem um **critério de aceite verificável**.
> Granularidade alta nas Fases 0–5 (onde o projeto começa); progressivamente mais leve no Tier 3.

---

## 0. Princípios que guiam o sequenciamento

1. **Ordem de construção ≠ ordem de valor.** Os grupos de prioridade não são a ordem de build. O parser (último item do Tier 1) destrava metade do Tier 2 e quase todo o Tier 3 → sobe na fila.
2. **Fonte é o coração — mas não reimplemente o shaper.** Aproveite `rustybuzz`/`allsorts` (shaping), `ttf-parser` (parsing), `fontations`/`klippa`/`allsorts` (subsetting). Seu trabalho é integrar + embutir corretamente no PDF.
3. **Fatias verticais cedo.** A meta de cada fase inicial é um PDF que abre e valida, não um módulo isolado perfeito. O primeiro milestone integrável (página em branco válida) força o pipeline inteiro a existir.
4. **Porte = decisão de API agora.** Mantenha a superfície pública FFI-friendly (handles opacos) desde o início se a meta for núcleo único + bindings.
5. **Mensurável = validador externo + corpus dourado + regressão visual.** Toda tarefa "termina" quando uma ferramenta externa diz pass.

---

## 1. Decisão arquitetural inicial (resolver na Fase 0)

### 1.1 Estratégia de porte — DECIDIDO: núcleo Rust + C ABI + bindings finos

**Arquitetura de DUAS camadas (não uma):**
- **Núcleo idiomático** (`pdf`, `cos`, `fonts`, ...) — rico: `Result`, builders, generics, enums com dados, lifetimes. É o que o usuário Rust consome. **NÃO restringir.**
- **Crate `ffi`** — camada fina, a *única* que cruza a fronteira. Handle-based, `extern "C"`, traduz o núcleo pro mundo C. **Só ela** obedece às regras da fronteira (§1.4).

> Erro a evitar: "FFI-ficar" o núcleo inteiro. Isso estraga a API Rust à toa. A disciplina vale só na borda.

**Substrato de distribuição:** C ABI escrito à mão + `cbindgen` (header) = espinha universal — toda linguagem faz FFI em C (Go, PHP, Ruby, Java, C#, Elixir...). `wasm-bindgen` = trilha separada p/ JS/web. UniFFI **não** será usado como fronteira primária (acopla ao modelo dele, não cobre C/Go; é ou-exclusivo com o C ABI). Reavaliar UniFFI só se mobile virar prioridade.

### 1.2 Estrutura de crates (workspace Cargo)
- `cos` — modelo de objetos + serialização
- `parser` — leitura de PDF existente
- `writer` — geração de documento/xref/trailer
- `graphics` — content stream + estado gráfico + cores
- `fonts` — parsing, embedding, subsetting, shaping
- `images` — JPEG/PNG
- `layout` — motor de fluxo de alto nível (L2): parágrafos com quebra automática, tabelas, header/footer, paginação
- `pdf` — API de alto nível idiomática (orquestra as anteriores) — **rica, não restrita**
- `ffi` — fronteira C ABI: **única camada restrita** (handles opacos, `extern "C"`)
- `testkit` — corpus, validadores, regressão visual

> Bindings por linguagem (`bindings/python`, `bindings/go`, ...) consomem o header gerado por `cbindgen` a partir da crate `ffi`.

### 1.2.1 Camadas de DX (facilidade para quem usa)

A API fluente **não atravessa o C ABI** — ela é reconstruída em cada lado. Três níveis, e "fácil" mora no L2:

- **L0** — operadores COS / content stream crus (interno, power user).
- **L1** — primitivas ergonômicas: "texto aqui, imagem ali, retângulo".
- **L2** — alto nível: parágrafos auto-quebrados, tabelas, header/footer, paginação automática. **É aqui que a DX vive.**

Cada binding tem **duas sub-camadas**: (a) declarações FFI cruas (geradas por `cbindgen`/bindgen); (b) **wrapper idiomático escrito à mão** que esconde handles e checagem de erro e expõe algo nativo da linguagem (context manager em Python, `defer Close()` em Go). Ninguém programa contra o C ABI direto.

> **C ABI não pode ser "chatty":** passe *runs* de texto, structs de config e arrays numa chamada só — nunca glifo a glifo. Granularidade do C ABI = "operação", não "primitiva". Um parágrafo = 1 chamada, não 200.

Alvo de DX (Rust):
```rust
let mut doc = Document::new();        // A4, defaults sensatos
doc.page()
   .heading("Fatura #42").size(20)
   .paragraph(texto_longo)            // quebra de linha automática (L2)
   .image("logo.png")?.fit_width()
   .table(linhas);                    // paginação automática (L2)
doc.save("fatura.pdf")?;
```

### 1.2.2 Modelo de concorrência — DECIDIDO: `Send`, não `Sync`

Cobre os casos reais: (1) gerar muitos documentos em paralelo num thread pool e (2) mover um `Document` entre threads. **Não** cobre mutar o mesmo documento de várias threads ao mesmo tempo (caso raro). Contrato no C ABI: *"um handle pode ser usado de qualquer thread, mas não de duas simultaneamente sem sincronização externa"*.

**Impacto no núcleo (decidir no commit 1):**
- Grafo de objetos em **arena/slab** (`Vec` + índices), **não** `Rc`/`RefCell`. Casa com o modelo PDF (referência indireta = "objeto N"), é naturalmente `Send`, melhor localidade de cache.
- Qualquer cache global (fontes etc.) é `Sync` (`OnceLock`/`Mutex`) ou não existe.
- Sem estado global mutável.

### 1.3 Verificação de licenças (produto pago)
Confirmar antes de adotar — a maioria é MIT/Apache (ok p/ comercial): `ttf-parser`, `rustybuzz`, `allsorts` (Apache-2.0), `fontations`, `flate2`/`miniz_oxide`, `image`/`png`, RustCrypto, `unicode-bidi`. **Registrar a licença de cada dependência num `LICENSES.md`.**

### 1.4 Regras da fronteira FFI (valem só na crate `ffi`)

- **Handles opacos** (`*mut Document`), nunca structs Rust expostos.
- **Proibido cruzar:** generics, lifetimes, traits, enums-com-dados, `Result`, e **panic** (UB).
- **Erros:** código de retorno `#[repr(C)] PdfStatus` + *last-error* thread-local com getter (`pdf_last_error_message`).
- **`catch_unwind` em TODO export** (ou `panic = "abort"` na cdylib). Panic nunca escapa.
- **Memória:** quem cria, libera. Todo handle/buffer tem seu `*_free`. Strings UTF-8 com `len` explícito.
- **Saída do PDF:** `(ptr, len)` + free, **ou** callback de escrita (preferível p/ streaming/arquivos grandes).
- **Crate-type:** `["cdylib", "staticlib"]`. Header via `cbindgen` no build.
- **Versionar o C ABI**; manter a superfície exportada pequena e estável (menos superfície = menos binding por linguagem).
- **Thread-safety** documentada por handle (Send/Sync?).

---

## 2. Fase 0 — Fundação e tooling (~1–2 semanas)

| # | Tarefa | Critério de aceite |
|---|---|---|
| 0.1 | Setup workspace Cargo multi-crate | `cargo build` compila todos os crates vazios |
| 0.2 | CI (build, test, clippy, fmt, deny) | Pipeline verde no primeiro push |
| 0.3 | Montar corpus dourado de PDFs (simples, fontes variadas, formulários, criptografados, CJK, corrompidos) | ≥30 PDFs catalogados com metadados |
| 0.4 | Harness de validação externa: wrappers p/ `qpdf --check`, `mutool clean`, `verapdf` | `testkit::validate(path)` retorna pass/fail por validador |
| 0.5 | Regressão visual: render via pdf.js/mutool → PNG → diff perceptual | `assert_visual(pdf, baseline)` falha com diff > limiar |
| 0.6 | Decisão de porte (§1.1) registrada em ADR | ADR commitado |
| 0.7 | Crate `ffi` esqueleto + `cbindgen` gerando header no CI | `make header` produz `pdf.h` válido |
| 0.8 | Binding de referência mínimo (Python via `ctypes`) que carrega a `cdylib` | Importa e chama `pdf_version()` de fora |
| 0.9 | Doc `FFI_RULES.md` (§1.4) + lint/checklist de revisão p/ exports | Checklist no template de PR |
| 0.10 | Matriz de build por plataforma (linux/macos/win × x86_64/arm64) | `cdylib` + `staticlib` saem no CI p/ cada alvo |

---

## 3. Fase 1 — Modelo de objetos COS + serialização (núcleo do núcleo)

| # | Tarefa | Critério de aceite |
|---|---|---|
| 1.1 | Tipos COS: Null, Bool, Integer, Real, Name, String (literal+hex), Array, Dict, Stream, Reference | Testes de construção/igualdade por tipo |
| 1.2 | Serializador objeto→bytes p/ cada tipo | Round-trip canônico de cada tipo |
| 1.3 | Escape correto: strings literais `\(`, hex `<...>`, nomes `#XX` | Testes com casos-limite (parênteses aninhados, bytes não-ASCII) |
| 1.4 | Stream object com `/Length` (direto e indireto) | Stream serializa e `qpdf` aceita |
| 1.5 | Document model: header `%PDF-1.7`, body, xref clássico, trailer, `%%EOF` | Estrutura byte-correta |
| 1.6 | **Milestone: gerar PDF de 1 página em branco** | Abre em Acrobat **e** pdf.js **e** `qpdf --check` sem erro |
| 1.7 | **Dogfood da fronteira:** export `pdf_document_new`/`_save`/`_free` + binding de referência gera o mesmo PDF de fora | Python (ctypes) produz PDF idêntico ao da API Rust; sem leak (valgrind/asan) |

---

## 4. Fase 2 — Páginas, content stream e gráficos vetoriais

| # | Tarefa | Critério de aceite |
|---|---|---|
| 2.1 | Page tree (`/Pages`, `/Kids`, `/Count`), Page, MediaBox, sistema de coordenadas | PDF multipágina válido |
| 2.2 | Content stream builder (operadores) | Stream sintaticamente válido p/ `mutool` |
| 2.3 | Estado gráfico: `q`/`Q`, `cm` (CTM), `w` | Transformações aninhadas corretas |
| 2.4 | Cores device: `rg`/`RG`, `g`/`G`, `k`/`K` (RGB, Gray, CMYK) | Cores corretas vs referência |
| 2.5 | Path: `m`, `l`, `c`, `v`, `y`, `re`, `h` | — |
| 2.6 | Pintura: `S`, `f`, `f*`, `B`, `n` + clipping `W`/`W*` | — |
| 2.7 | **Milestone: PDF com linhas/retângulos/curvas coloridos** | Regressão visual pixel-correta vs baseline |

---

## 5. Fase 3 — Texto e fontes (o coração — máxima granularidade)

### Sub-épico A — Fontes simples Latin
| # | Tarefa | Critério de aceite |
|---|---|---|
| 3A.1 | Parsear tabelas TTF/OTF via `ttf-parser`: head, hhea, hmtx, maxp, cmap, name, OS/2, glyf/loca ou CFF | Métricas extraídas batem com `ttx` |
| 3A.2 | Embutir fonte completa: `FontFile2` (TrueType) / `FontFile3` (CFF) | Fonte aparece embutida em `mutool info` |
| 3A.3 | Font dict simples + `WinAnsiEncoding` + `FirstChar`/`LastChar`/`Widths` | — |
| 3A.4 | Operadores de texto: `BT`/`ET`, `Tf`, `Td`/`TD`/`Tm`, `Tj`/`TJ`, `Tc`, `Tw`, `Tz`, `TL`, `Ts` | — |
| 3A.5 | **Milestone: "Hello World" embutido** | `pdftotext`/`mutool` extraem o texto corretamente |

### Sub-épico B — Unicode / Type0 / CID
| # | Tarefa | Critério de aceite |
|---|---|---|
| 3B.1 | Type0 font + CIDFontType2 + `Identity-H` | — |
| 3B.2 | `CIDToGIDMap` | — |
| 3B.3 | `W` array de larguras CID | Larguras corretas no render |
| 3B.4 | `ToUnicode` CMap | Texto com acentos (é, ñ, ç) **extrai** corretamente |

### Sub-épico C — Subsetting
| # | Tarefa | Critério de aceite |
|---|---|---|
| 3C.1 | Coletar glyphs usados; remapear GIDs | — |
| 3C.2 | Reconstruir loca/glyf/hmtx/cmap (ou usar `klippa`/`allsorts` subsetter) | — |
| 3C.3 | **Milestone: subset** | Arquivo ≥70% menor, ainda renderiza e extrai texto |

### Sub-épico D — Shaping e quebra de linha (Latin)
| # | Tarefa | Critério de aceite |
|---|---|---|
| 3D.1 | Integrar `rustybuzz`/`allsorts` p/ shaping Latin (kerning GPOS/kern, ligaduras) | Kerning vs baseline HarfBuzz |
| 3D.2 | Quebra de linha gulosa | Parágrafo quebra sem estourar margem |
| 3D.3 | (Opcional v2) Knuth-Plass p/ justificação ótima | Rios reduzidos vs guloso |

### Sub-épico E — Scripts complexos (o diferencial real)
| # | Tarefa | Critério de aceite |
|---|---|---|
| 3E.1 | Árabe: formas contextuais/joining (via shaper) | Render vs baseline HarfBuzz |
| 3E.2 | Devanagari/Indic: reordenação (via shaper) | Render vs baseline |
| 3E.3 | BiDi via `unicode-bidi` | Texto misto LTR/RTL na ordem visual correta |
| 3E.4 | CJK: fontes grandes, subsetting CJK, vertical opcional | Amostra CJK renderiza e extrai |

### Sub-épico F — L2 básico (o maior salto de DX; depende de 3D)
| # | Tarefa | Critério de aceite |
|---|---|---|
| 3F.1 | `paragraph(texto)` com quebra de linha automática dentro de uma caixa/coluna | Texto longo flui sem estourar margem, sem posicionar glifo na mão |
| 3F.2 | Alinhamento (left/right/center/justify) + espaçamento de linha | Render vs baseline |
| 3F.3 | Estilo inline (negrito/itálico/cor/tamanho) dentro do parágrafo | Trechos com estilos mistos corretos |
| 3F.4 | **Milestone DX:** snippet-alvo (`doc.page().heading().paragraph()...`) funciona ponta a ponta | Exemplo do §1.2.1 compila e gera PDF válido |

> Tabelas, header/footer e paginação automática continuam no item 7.6 (Tier 3) — esta fatia entrega só o fluxo de parágrafo, que já é o maior ganho de usabilidade.

---

## 6. Fase 4 — Imagens

| # | Tarefa | Critério de aceite |
|---|---|---|
| 4.1 | JPEG: embed direto via `DCTDecode` (ler SOF p/ dimensões/componentes, sem decodificar) | JPEG aparece; arquivo não recomprime |
| 4.2 | Image XObject + operador `Do` | — |
| 4.3 | PNG: decodificar (`png`), recodificar `FlateDecode` | PNG opaco correto |
| 4.4 | PNG paleta → indexed; alpha → `SMask`; 16-bit | PNG com transparência correto via regressão visual |

---

## 7. Fase 5 — Parser de PDF existente (destrava Tier 2 e 3)

| # | Tarefa | Critério de aceite |
|---|---|---|
| 5.1 | Tokenizer/lexer | Tokeniza corpus sem panic |
| 5.2 | Parser de objetos indiretos | Objetos do corpus parseiam |
| 5.3 | xref clássico + trailer + `/Root` + `/Info` | Catálogo localizado em todo o corpus |
| 5.4 | Filtros: `FlateDecode`, `LZWDecode`, `ASCIIHexDecode`, `ASCII85Decode`, `RunLengthDecode` | Streams decodificam byte-correto |
| 5.5 | xref streams (cross-reference streams) | PDFs 1.5+ parseiam |
| 5.6 | Object streams (objetos comprimidos) | — |
| 5.7 | PDFs lineares/híbridos | — |
| 5.8 | Recuperação: reconstruir xref por scan de `obj` quando corrompido | PDFs corrompidos do corpus abrem |
| 5.9 | Leitura de criptografado (RC4 + AES, standard handler) | PDF protegido por senha abre com senha |
| 5.10 | **Milestone: round-trip do corpus** | Round-trip de N PDFs passa `qpdf --check` |

---

## 8. Fase 6 — Manipulação (Tier 2 — maioria dos casos comerciais)

| # | Tarefa | Critério de aceite |
|---|---|---|
| 6.1 | Merge: renumerar objetos, mesclar page trees, dedupe de recursos | N PDFs → 1; `qpdf --check` passa |
| 6.2 | Split / extrair páginas com dependências | Páginas extraídas abrem isoladas |
| 6.3 | Rotacionar (`/Rotate`), reordenar, deletar páginas | — |
| 6.4 | **Extração de texto** (content stream → glyphs → Unicode via ToUnicode/encoding; inferir espaços/linhas por posição; ordem de leitura) | Texto vs `pdftotext` em corpus; foco em qualidade p/ RAG |
| 6.5 | Metadados: Info dict + XMP (ler/escrever) | Round-trip de metadados |
| 6.6 | Marca d'água / stamp / overlay | Overlay aparece sem corromper original |
| 6.7 | Preencher AcroForm (field dicts, widgets, appearance streams, `NeedAppearances`) | Formulário preenchido renderiza valores |
| 6.8 | Compressão/otimização (object streams, dedupe, recompress, remover não-usados) | Redução de tamanho mensurável; `qpdf --check` passa |

---

## 9. Fase 7 — Diferenciadores (Tier 3 — escopo leve, vai mudar)

| # | Tarefa | Critério de aceite |
|---|---|---|
| 7.1 | Assinatura: `ByteRange` placeholder + incremental update + PKCS#7 detached | Acrobat valida a assinatura |
| 7.2 | Timestamp RFC 3161 + níveis PAdES (B-B → B-LTA) | Validação PAdES passa |
| 7.3 | Criptografia: standard handler, AES-256, flags de permissão | PDF protegido respeita permissões |
| 7.4 | PDF/A: conversão (embed tudo, OutputIntent, XMP com id PDF/A, sem transparência p/ A-1) | **`verapdf` valida** o nível alvo |
| 7.5 | Tagged PDF / PDF/UA: structure tree, marked content, role map | `verapdf`/PAC validam acessibilidade |
| 7.6 | Engine de layout alto-nível: **estende o L2 básico (3F)** com tabelas, header/footer, paginação automática | Relatório multipágina com tabela quebra corretamente |
| 7.7 | HTML/CSS → PDF (subconjunto definido) | Conjunto de templates-alvo renderiza fiel |

> **Alerta de escopo em 7.7:** HTML/CSS→PDF é, na prática, um motor de layout de browser (box model, cascata, fluxo de texto). É a feature mais pedida do mundo *e* a de maior custo. Recomendação: definir um **subconjunto explícito de CSS** (o que faturas/relatórios reais usam) em vez de "CSS completo", ou avaliar wrapping. Tratar como produto à parte, não como tarefa.

---

## 10. Template de "Definition of Done" (toda tarefa)

- [ ] Testes unitários cobrindo casos-limite
- [ ] Passa no(s) validador(es) externo(s) aplicável(is) (`qpdf`/`mutool`/`verapdf`)
- [ ] Regressão visual (se gera conteúdo renderizável)
- [ ] Round-trip no corpus dourado (se toca parser/writer)
- [ ] **Núcleo permanece `Send`** (sem `Rc`/`RefCell` no grafo; cache global `Sync`)
- [ ] **Tipos do núcleo NÃO vazam pela borda** — só a crate `ffi` toca a fronteira (handles opacos)
- [ ] Licença de qualquer nova dependência registrada

---

## 11. Sugestão de ordem de fatiamento para "começar granular"

1. Fase 0 (tooling) — sem isso nada é mensurável.
2. Fases 1–2 até o **PDF colorido válido** + fronteira dogfoodada (1.7) — primeira fatia vertical.
3. Fase 3 Sub-épicos A→B→C — saia de "Hello World" para "Unicode subsetado". É aqui que você já passa a maioria das libs grátis.
4. Fase 3 Sub-épicos D→**F** — kerning, quebra de linha e o **L2 básico (parágrafo)**. Maior salto de DX; entrega o snippet-alvo do §1.2.1.
5. Fase 5 (parser) — destrava o Tier 2 inteiro.
6. Fase 6 priorizando **6.4 (extração de texto)** — maior demanda atual (RAG/IA).
7. Sub-épico 3E (scripts complexos) em paralelo conforme clientes pedirem.
8. Tier 3 conforme a demanda real (jurídico → assinatura; governo → PDF/A + PDF/UA; faturas/relatórios → 7.6 estendendo o 3F).
