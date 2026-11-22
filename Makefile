.PHONY: ci backend-ci frontend-ci terraform-check deploy

ci: backend-ci frontend-ci terraform-check

backend-ci:
	cd backend && cargo fmt --check
	cd backend && cargo clippy --locked --all-targets -- -D warnings -W clippy::cognitive_complexity
	cd backend && cargo test --locked

frontend-ci:
	cd frontend && pnpm install --frozen-lockfile
	cd frontend && pnpm exec eslint .
	cd frontend && pnpm typecheck
	cd frontend && pnpm test

terraform-check:
	terraform fmt -check -recursive infrastructure/terraform

deploy:
	./scripts/deploy.sh
