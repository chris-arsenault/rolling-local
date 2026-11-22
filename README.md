# Rolling Local

Rolling Local is a food truck discovery project. The Prescott, Arizona pilot is designed around paid operator accounts, quick location and menu sharing, and a free public map and directory.

The public website is at [rolling-local.ahara.io](https://rolling-local.ahara.io). The operator app targets iOS and Android. AWS Lambda serves the API behind Ahara’s shared load balancer, and CloudFront serves the consumer site and published menus from private S3.

## Local checks

Run `make ci` with Rust, Node, pnpm and Terraform installed. See the deployment guide for native build commands and the Ahara workflow.

| Topic | Start here |
| --- | --- |
| Browse the documentation | [Documentation index](docs/README.md) |
| View the architecture and UI mockups | [MVP design reference](docs/architecture.md) |
| Find the MVP scope and Ahara hosting guidance | [Backlog](docs/backlog.md) |
| Work in this repository | [Agent guide](AGENTS.md) |
| Browse releases | [Changelog](CHANGELOG.md) |
| Build and deploy | [Deployment](docs/deployment.md) |

## License

[MIT](LICENSE)
