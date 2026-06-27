#!/usr/bin/env bash
# Republica o site rust-pdf no VPS (k3s). Rode do seu Mac, da raiz do repo ou de qualquer lugar:
#
#   ./site/scripts/deploy.sh            # rsync + build + import + rollout + verifica
#   ./site/scripts/deploy.sh --logs     # idem, e segue os logs no fim
#   SKIP_RSYNC=1 ./site/scripts/deploy.sh   # só rebuild+rollout (sem sincronizar código)
#
# NÃO toca em segredos: o ~/site/.env.deploy (TEST_CHECKOUT/Stripe/SendGrid/seed) é
# excluído do rsync, então a config de produção do VPS é preservada. Para mudar env,
# edite o .env.deploy no VPS e rode `kubectl create secret ... | apply` (ver INFRA.md).
set -euo pipefail

# ---- Config (sobrescreva por env var se precisar) --------------------------
VPS="${VPS:-edivan@VPS_HOST}"      # destino SSH
NS="${NS:-rustpdf}"                       # namespace k8s
DEPLOY="${DEPLOY:-rustpdf-site}"          # nome do Deployment
IMAGE="${IMAGE:-rustpdf-site:test}"       # tag da imagem (fixa; imagePullPolicy: Never)
REMOTE_DIR="${REMOTE_DIR:-~/rust-pdf}"    # build context no VPS
URL="${URL:-https://rustpdf.dev}"         # URL pública p/ smoke test

# Raiz do repo = dois níveis acima deste script (site/scripts/ -> repo/)
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

say() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
die() { printf '\n\033[1;31mERRO: %s\033[0m\n' "$*" >&2; exit 1; }

# ---- 0. Atualizar o download público do Delphi (zip + sha em public/downloads) --
# Puxa o artefato do GitHub Release (gerado pelo workflow release-delphi) para
# site/public/downloads/, que o rsync abaixo leva para a imagem. Pule com
# SKIP_DOWNLOADS=1 (ex.: antes de existir o primeiro release).
if [ "${SKIP_DOWNLOADS:-0}" != "1" ]; then
  say "sincronizando download do Delphi (GitHub Release -> site/public/downloads)"
  if ! "$ROOT/site/scripts/sync-delphi-download.sh"; then
    if ls "$ROOT"/site/public/downloads/rustpdf-delphi-*.zip >/dev/null 2>&1; then
      printf '\033[1;33m   aviso: sync falhou; usando o zip já presente em public/downloads\033[0m\n'
    else
      die "não consegui obter o zip do Delphi e não há cópia local. Publique o release (tag delphi-v*) ou rode com SKIP_DOWNLOADS=1"
    fi
  fi
else
  say "SKIP_DOWNLOADS=1 — pulando sync do download do Delphi"
fi

# ---- 0b. Atualizar o download público do Swift (xcframework + pacote) --------
# Mesmo fluxo do Delphi, mas não-fatal: se ainda não há release swift-v*, segue
# usando o zip local (se existir) ou simplesmente sem o download Swift.
if [ "${SKIP_DOWNLOADS:-0}" != "1" ]; then
  say "sincronizando download do Swift (GitHub Release -> site/public/downloads)"
  if ! "$ROOT/site/scripts/sync-swift-download.sh"; then
    if ls "$ROOT"/site/public/downloads/rustpdf-swift-*.zip >/dev/null 2>&1; then
      printf '\033[1;33m   aviso: sync Swift falhou; usando o zip já presente em public/downloads\033[0m\n'
    else
      printf '\033[1;33m   aviso: sem release Swift ainda; seguindo sem o download Swift\033[0m\n'
    fi
  fi
fi

# ---- 1. Sincronizar o build context ----------------------------------------
if [ "${SKIP_RSYNC:-0}" != "1" ]; then
  say "rsync do código para $VPS:$REMOTE_DIR (segredos preservados)"
  rsync -az --delete \
    --exclude 'target' --exclude 'node_modules' --exclude '.git' \
    --exclude 'site/data' --exclude 'site/.env' --exclude 'site/.env.deploy' \
    Cargo.toml Cargo.lock crates site \
    "$VPS:$REMOTE_DIR/"
else
  say "SKIP_RSYNC=1 — pulando sincronização"
fi

# ---- 2/3/4. Build + import no k3s + rollout (tudo no VPS) -------------------
say "build da imagem + import no containerd do k3s + rollout (no VPS)"
ssh "$VPS" "NS='$NS' DEPLOY='$DEPLOY' IMAGE='$IMAGE' REMOTE_DIR='$REMOTE_DIR' bash -s" <<'REMOTE'
set -euo pipefail
eval REMOTE_DIR="$REMOTE_DIR"   # expande o ~

echo "--- guard: .env.deploy presente?"
test -f "$REMOTE_DIR/site/.env.deploy" || { echo "ERRO: $REMOTE_DIR/site/.env.deploy sumiu — abortando"; exit 1; }

echo "--- docker build"
cd "$REMOTE_DIR"
sudo docker build -f site/Dockerfile -t "$IMAGE" . >/tmp/rustpdf-build.log 2>&1 \
  || { echo "build falhou:"; tail -30 /tmp/rustpdf-build.log; exit 1; }
echo "build ok"

echo "--- import no k3s containerd"
sudo docker save "$IMAGE" -o /tmp/rustpdf-site.tar
sudo k3s ctr images import /tmp/rustpdf-site.tar | tail -1
sudo rm -f /tmp/rustpdf-site.tar

echo "--- rollout"
sudo kubectl -n "$NS" rollout restart "deploy/$DEPLOY"
sudo kubectl -n "$NS" rollout status "deploy/$DEPLOY" --timeout=90s
REMOTE

# ---- 5. Verificação ---------------------------------------------------------
say "verificação"
ssh "$VPS" "NS='$NS' DEPLOY='$DEPLOY' bash -s" <<'REMOTE'
set -euo pipefail
POD=$(sudo kubectl -n "$NS" get pod -l app="$DEPLOY" -o jsonpath='{.items[0].metadata.name}')
echo "pod: $POD ($(sudo kubectl -n "$NS" get pod "$POD" -o jsonpath='{.status.phase} ready={.status.containerStatuses[0].ready}'))"
echo -n "modo checkout: "
sudo kubectl -n "$NS" exec "$POD" -- node -e 'fetch("http://127.0.0.1:3000/api/checkout",{method:"POST",headers:{"content-type":"application/json"},body:JSON.stringify({tier:"pro"})}).then(r=>r.json()).then(j=>console.log(j.test?"TEST (simulado!)":"produção (cs_live)")).catch(e=>console.log("erro:",e.message))'
REMOTE

code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 15 "$URL" || echo 000)
lm=$(curl -s -o /dev/null -w '%header{last-modified}' --max-time 15 "$URL" || true)
health=$(curl -s --max-time 15 "$URL/healthz" || true)
echo "público $URL -> HTTP $code | last-modified: $lm | healthz: $health"
[ "$code" = "200" ] || die "site público não retornou 200 (HTTP $code)"

say "✅ deploy concluído"

if [ "${1:-}" = "--logs" ]; then
  say "logs (Ctrl-C para sair)"
  ssh -t "$VPS" "sudo kubectl -n '$NS' logs -f deploy/'$DEPLOY' --tail=20"
fi
