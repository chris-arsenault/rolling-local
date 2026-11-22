# Build and deployment

| Part | Local command from the repository root |
| --- | --- |
| API tests | `cargo test --locked --manifest-path backend/Cargo.toml` |
| Consumer production build | `pnpm --dir frontend install --frozen-lockfile && pnpm --dir frontend build` |
| Android debug build | `gradle -p mobile/android :app:assembleDebug :app:lintDebug` with Java 17, Gradle 8.9 and Android SDK 35 |
| iOS simulator build | Run `xcodegen generate` in `mobile/ios`, then build the `RollingLocal` scheme for iOS Simulator in Xcode |

The Android and iOS debug builds accept `ROLLING_LOCAL_API` as a build or launch setting for a test API origin. The consumer app uses the value in the CloudFront site's `config.js` and accepts `VITE_API_ORIGIN` for local builds.

## AWS

Merges to `main` run the iOS build, Ahara’s reusable CI for the API, Android and the web, Terraform apply, and smoke tests for the deployed endpoints. The local equivalent for an authorized operator is `with-cred -- make deploy` in a brokered environment or `make deploy` with ordinary AWS credentials.

The platform registration is in `ahara-infra/infrastructure/terraform/control/project-rolling-local.tf`. The application stack is in `infrastructure/terraform`. The consumer site uses Ahara’s website module and private S3 with server-side encryption. Menu uploads are staged in a separate private bucket, and the site distribution reads the published JPEG prefix.

## Operator access

The application Terraform publishes the operator’s shared Cognito client ID under Ahara’s app discovery path. After creating or changing that client, run the standard `ahara-infra` deployment so the shared pre-authentication allowlist picks it up. Grant each operator the `rolling-local` application entitlement in the Ahara account portal and have them complete the shared MFA enrollment. The public site requires no account.

The Apple device build uses the app and widget IDs in `mobile/ios/project.yml` and the listed App Group. Set a team with signing access to those identifiers before sideloading. Android’s debug build can be installed from Android Studio or with `adb install`. See the [MVP operating contract](mvp-contract.md) for the validated API behavior and account workflow.
