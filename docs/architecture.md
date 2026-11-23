# Rolling Local design reference

The architecture canvas covers the operator app, location sharing, and public discovery. The registration canvas shows the direct operator entry, shared Auth0 sign-up, truck review, manual entry, and the public About page.

The deployment design routes the operator API and public discovery reads through Ahara’s shared application load balancer to Lambda functions in the shared VPC. CloudFront serves the public site from private S3. See [Ahara hosting](backlog.md#ahara-hosting) for the platform integration requirements.

| Diagram | Rendered PNG | Editable Arconis |
| --- | --- | --- |
| Architecture and operator/consumer UI | [PNG](diagrams/food-truck-discovery.png) | [Source](diagrams/food-truck-discovery.arconis) |
| Truck registration, administration, and About | [PNG](diagrams/truck-registration.png) | [Source](diagrams/truck-registration.arconis) |

## Regenerate a diagram

In a local Arconis canvas, use **File → Import document** to open the `.arconis` source. Show all layers, clear the selection, and use **File → Export PNG**. Save each PNG next to its editable source with the same base name.
