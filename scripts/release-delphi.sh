#!/usr/bin/env bash
# Release completo do pacote Delphi, ponta a ponta e sem passo manual:
#
#   scripts/release-delphi.sh [versao] [opcoes]
#
#   (bump da versao do workspace, se informada) -> commit -> tag delphi-v<versao>
#   -> push -> espera o CI (release-delphi) publicar o GitHub Release
#   -> deploy do site (site/scripts/deploy.sh puxa o zip e republica)
#   -> verifica o download e a pagina publica.
#
# A versao aparece UMA vez (Cargo.toml do workspace). A pagina /docs/delphi.html
# injeta essa versao em runtime (placeholder __DELPHI_VERSION__), entao nao ha
# HTML para editar a cada release.
#
# Opcoes:
#   --skip-ci-wait   nao espera o workflow; assume o Release ja existe.
#   --skip-deploy    para apos publicar o Release (nao mexe no site).
#   --force-tag      recria a tag se ela ja existir.
#
# Requisitos: git + gh (autenticado), e acesso de deploy (SSH ao VPS) se for
# fazer o deploy. Rode da raiz do repo.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

say()  { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
warn() { printf '\033[1;33m   %s\033[0m\n' "$*"; }
die()  { printf '\n\033[1;31mERRO: %s\033[0m\n' "$*" >&2; exit 1; }

# ---- args ------------------------------------------------------------------
VERSION=""
SKIP_CI_WAIT=0; SKIP_DEPLOY=0; FORCE_TAG=0
for a in "$@"; do
  case "$a" in
    --skip-ci-wait) SKIP_CI_WAIT=1 ;;
    --skip-deploy)  SKIP_DEPLOY=1 ;;
    --force-tag)    FORCE_TAG=1 ;;
    -*)             die "opcao desconhecida: $a" ;;
    *)              VERSION="$a" ;;
  esac
done

command -v git >/dev/null || die "git nao encontrado"
command -v gh  >/dev/null || die "gh (GitHub CLI) nao encontrado — necessario para publicar/baixar o Release do repo privado"

CUR="$(grep -m1 -E '^version = ' Cargo.toml | sed -E 's/version = "(.*)"/\1/')"
VERSION="${VERSION:-$CUR}"
echo "$VERSION" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$' || die "versao invalida: '$VERSION' (use X.Y.Z)"
TAG="delphi-v${VERSION}"
BRANCH="$(git rev-parse --abbrev-ref HEAD)"

say "release Delphi v${VERSION} (atual no Cargo.toml: ${CUR}; branch: ${BRANCH})"

# ---- 1. bump da versao do workspace (se mudou) -----------------------------
if [ "$VERSION" != "$CUR" ]; then
  git diff --quiet && git diff --cached --quiet || die "arvore suja — faca commit/stash antes de fazer bump de versao"
  say "bump da versao $CUR -> $VERSION no Cargo.toml"
  tmp="$(mktemp)"
  awk -v v="$VERSION" 'BEGIN{d=0} /^version = "/ && !d {sub(/"[^"]*"/, "\"" v "\""); d=1} {print}' Cargo.toml > "$tmp"
  mv "$tmp" Cargo.toml
  grep -qE "^version = \"${VERSION}\"" Cargo.toml || die "falha ao escrever a versao no Cargo.toml"
  git add Cargo.toml
  git commit -m "release: delphi v${VERSION}"
fi

# ---- 2. tag + push ---------------------------------------------------------
if git rev-parse "$TAG" >/dev/null 2>&1; then
  [ "$FORCE_TAG" = "1" ] || die "tag $TAG ja existe (use --force-tag para recriar)"
  warn "recriando tag $TAG"
  git tag -d "$TAG"
  git push origin ":refs/tags/$TAG" || true
fi
say "criando e enviando a tag $TAG"
git tag "$TAG"
git push origin "$BRANCH"
git push origin "$TAG"

# ---- 3. esperar o CI publicar o Release ------------------------------------
if [ "$SKIP_CI_WAIT" != "1" ]; then
  say "aguardando o workflow release-delphi (CI builda libs + publica o Release)"
  sleep 8   # deixa o run registrar
  RID=""
  for _ in 1 2 3 4 5 6; do
    RID="$(gh run list --workflow release-delphi.yml -L 1 --json databaseId -q '.[0].databaseId' 2>/dev/null || true)"
    [ -n "$RID" ] && break
    sleep 5
  done
  [ -n "$RID" ] || die "nao achei o run do workflow release-delphi — veja em Actions e rode com --skip-ci-wait depois"
  gh run watch "$RID" --exit-status || die "o workflow release-delphi falhou — veja os logs (gh run view $RID --log-failed)"
  say "Release $TAG publicado"
else
  warn "--skip-ci-wait: assumindo que o Release $TAG ja existe"
fi

# ---- 4. deploy do site (puxa o zip do Release e republica) -----------------
if [ "$SKIP_DEPLOY" != "1" ]; then
  say "deploy do site (sync do download + rebuild + rollout)"
  ./site/scripts/deploy.sh
else
  warn "--skip-deploy: pulando o deploy do site"
  warn "para publicar o download depois: ./site/scripts/deploy.sh"
fi

# ---- 5. verificacao publica ------------------------------------------------
if [ "$SKIP_DEPLOY" != "1" ]; then
  URL="${URL:-https://rustpdf.dev}"
  say "verificacao publica"
  code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 20 "$URL/downloads/rustpdf-delphi-${VERSION}.zip" || echo 000)
  [ "$code" = "200" ] && echo "  download  -> $URL/downloads/rustpdf-delphi-${VERSION}.zip (HTTP 200)" \
                      || warn "download retornou HTTP $code (CDN/propagacao? confira)"
  if curl -s --max-time 20 "$URL/docs/delphi.html" | grep -q "rustpdf-delphi-${VERSION}.zip"; then
    echo "  pagina    -> /docs/delphi.html aponta para v${VERSION}"
  else
    warn "a pagina ainda nao mostra v${VERSION} (cache do container? confira o rollout)"
  fi
fi

say "✅ release Delphi v${VERSION} concluido"
