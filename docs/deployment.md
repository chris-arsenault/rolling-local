# Build and deployment

| Part | Local command from the repository root |
| --- | --- |
| API tests | `cargo test --locked --manifest-path backend/Cargo.toml` |
| Website production build | `pnpm --dir frontend install --frozen-lockfile && pnpm --dir frontend build` |
| Android debug build | `gradle -p mobile/android :app:assembleDebug :app:lintDebug` with Java 17, Gradle 8.9 and Android SDK 35 |
| iOS simulator build | Run `xcodegen generate` in `mobile/ios`, then build the `RollingLocal` scheme for iOS Simulator in Xcode |

The web app reads its API origin from the website module’s `config.js`. For Android development, set the `rollingLocalApiOrigin` Gradle project property. For iOS development, set the `ROLLING_LOCAL_API_ORIGIN` Xcode build setting, which populates the app’s Info.plist.

## AWS

Merges to `main` run the iOS and Android builds, followed by Ahara’s reusable CI for the API and web, Terraform apply, and smoke tests for the deployed endpoints. Terraform publishes the Lambda settings as JSON in Parameter Store at `/rolling-local/application` and uses Ahara’s existing Auth0 provider configuration.

The platform registration is in `ahara-infra/infrastructure/terraform/control/project-rolling-local.tf`. The application stack is in `infrastructure/terraform`. Ahara’s website module serves one React shell from private S3, including for direct URLs and Auth0 callbacks. React selects the route from the path. The site distribution also reads the published JPEG prefix from the private menu bucket.

## Operator access

Share `https://rolling-local.ahara.io/register` directly with operators. They use an Ahara Auth0 account, submit the truck form, and return to that URL for their review status. The owner uses `https://rolling-local.ahara.io/admin` to review and manage listings. The discovery homepage serves consumers and links to the informational About page.

To enable owner access, set the String parameter `/rolling-local/admin-subject` in AWS Systems Manager Parameter Store in `us-east-1` to the owner’s verified Ahara Auth0 user ID, also called the `sub` claim. The API reads the grant for admin requests. The owner setting is independent of application deployment.

Existing truck records keep their listing IDs and menu photos at cutover. They appear as unassigned, approved listings. To restore operator access, ask the operator to sign in from the registration link and share the account ID displayed on that page before submitting a new form. Confirm their identity and assign that ID to the existing truck in the owner screen. The operator can then start a fresh sharing session from the native app.

The Apple device build uses the app and widget IDs in `mobile/ios/project.yml` and the listed App Group. Set a team with signing access to those identifiers before sideloading. Android’s debug build can be installed from Android Studio or with `adb install`. See the [MVP operating contract](mvp-contract.md) for the API and account workflow.
