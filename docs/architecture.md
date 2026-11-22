# MVP design reference

The Arconis canvas records the MVP design for the operator app, location sharing, and the public map and directory. It includes operator settings, sharing status, menu capture, both compact widget layouts, and consumer map and directory mockups.

The deployment design routes the operator API and public discovery reads through Ahara’s shared application load balancer to Lambda functions in the shared VPC. CloudFront serves the public site from private S3. See [Ahara hosting](backlog.md#ahara-hosting) for the platform integration requirements.

The design reference is available in checkouts with the local Arconis export.

[Open the full-resolution PNG](diagrams/food-truck-discovery.png) · [Open the editable Arconis source](diagrams/food-truck-discovery.arconis)

## Regenerate the diagram

In a local Arconis canvas, use **File → Import document** to open the `.arconis` source. Show all layers, clear the selection, and use **File → Export PNG**. Save the resulting image as `docs/diagrams/food-truck-discovery.png`.
