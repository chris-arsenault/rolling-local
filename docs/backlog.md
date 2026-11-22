# Backlog

Delivery phases and acceptance criteria are in the Sulion plan **Food Truck Discovery & Operator Tracking** (`1e5d89fe-2b7a-4545-bee5-32a05223a975`).

## Prescott pilot

- Define onboarding and service for paid truck operators and free public browsing.
- Build the iOS and Android operator app with a compact home-screen sharing toggle, camera shortcut, and timestamped menu photos. Select between the two widget layouts in the [design reference](architecture.md).
- Publish fresh, approximate phone locations during manually started sessions. Enforce operating hours, the configured radius or temporary event override, and session expiry. Remove the public location when sharing stops.
- Build a free web map for active broadcasts and a name- and cuisine-searchable directory of all enrolled trucks. Show menus and active or offline status. Keep offline directory entries free of past phone coordinates.
- Exercise the complete backend, operator app, and consumer web app with simulated location during development. Sideload the integrated builds for real-device widget, background tracking, battery, and privacy checks before the Prescott pilot.

## Ahara hosting

- Place the operator API and public consumer site in AWS. Ahara reserves TrueNAS for owner-only workloads. See `../ahara/TRUENAS-DEPLOY.md` from the repository root.
- Use Ahara’s shared application load balancer and VPC for HTTP Lambda routes through the `alb-api` module. Validate operator JWTs at the load balancer and authorize truck-specific writes in Lambda. Keep the map and directory reads public.
- Evaluate the design’s DynamoDB candidate for truck profiles, broadcast sessions, and the latest eligible location. Ahara provides a project-scoped `dynamodb` deployer policy. For workloads that need PostgreSQL, Ahara provides a database on shared RDS.
- Serve the consumer site from private S3 through CloudFront using Ahara’s `website` module. Scope menu media access separately and use authenticated API requests to authorize direct uploads to S3. Keep active-location responses fresh enough to honor manual stop and automatic expiry.
- Use the shared identity service appropriate to operator enrollment. The integration guide routes managed accounts through Cognito and open sign-up through the shared Auth0 tenant. Consumers browse without signing in.
- Register the project’s OIDC deploy role in `ahara-infra` when preparing the deployment. Keep application Terraform in this repo and use the shared state bucket, project-scoped state, and the platform’s tag and SSM discovery. Use Ahara’s shared CI/CD workflow with a project `platform.yml` when the deployment is configured.

See `../ahara/INTEGRATION.md` for platform constraints and module setup, `../ahara/docs/architecture.md` for infrastructure ownership, and `../ahara/CI-WORKFLOW.md` for the deployment workflow. Paths are relative to this repository’s root.
