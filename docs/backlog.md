# Backlog

The [design reference](architecture.md) covers consumer discovery, operator registration, and owner review. Delivery work is tracked in the [Rolling Local registration plan](https://sulion.services.ahara.io/?plan=4884739e-45df-4e74-83f4-b533b98ee53c).

## Prescott pilot

- Exercise the complete backend, operator app, and consumer web app with simulated location during development. Sideload the integrated builds for real-device widget, background tracking, battery, and privacy checks before the Prescott pilot.

## Ahara hosting

- Place the operator API and public consumer site in AWS. Ahara reserves TrueNAS for owner-only workloads. See `../ahara/TRUENAS-DEPLOY.md` from the repository root.
- Use Ahara’s shared application load balancer and VPC for HTTP Lambda routes through the `alb-api` module. Validate operator JWTs at the load balancer and authorize truck-specific writes in Lambda. Keep the map and directory reads public.
- Store truck records, approval decisions, and the latest eligible session in DynamoDB with the project-scoped Ahara deployer policy.
- Serve the consumer site from private S3 through CloudFront using Ahara’s `website` module. Scope menu media access separately and use authenticated API requests to authorize direct uploads to S3. Keep active-location responses fresh enough to honor manual stop and automatic expiry.
- Use Ahara’s public Auth0 configuration for operator and admin sign-in. Consumers browse without signing in.
- Register the project’s OIDC deploy role in `ahara-infra` when preparing the deployment. Keep application Terraform in this repo and use the shared state bucket, project-scoped state, and the platform’s tag and SSM discovery. Use Ahara’s shared CI/CD workflow with a project `platform.yml` when the deployment is configured.

See `../ahara/INTEGRATION.md` for platform constraints and module setup, `../ahara/docs/architecture.md` for infrastructure ownership, and `../ahara/CI-WORKFLOW.md` for the deployment workflow. Paths are relative to this repository’s root.
