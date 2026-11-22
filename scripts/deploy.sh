#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
terraform_dir="$root/infrastructure/terraform"
state_bucket="${STATE_BUCKET:-tfstate-559098897826}"

(cd "$root/backend" && cargo lambda build --release --locked)
pnpm --dir "$root/frontend" install --frozen-lockfile
pnpm --dir "$root/frontend" build
terraform -chdir="$terraform_dir" init -reconfigure \
  -backend-config="bucket=$state_bucket" \
  -backend-config="use_lockfile=true"
terraform -chdir="$terraform_dir" apply -auto-approve
