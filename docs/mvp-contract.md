# MVP operating contract

## Operator enrollment

Operators open the direct registration link, sign in or sign up with Ahara Auth0, and submit their truck name and cuisine. A submission starts pending. The owner reviews it in Rolling Local and accepts or rejects it. The operator can return to the same link or use the native app to see the decision. The owner can also add trucks manually and choose when they appear publicly.

After approval, the assigned operator can set the time zone, weekly hours, and normal operating circle. They can set a separate circle and expiration for an event. Expiry restores the normal circle, and the routine settings are stored independently.

## Location and privacy

The compact widget offers a sharing toggle and a camera shortcut. Starting sharing requires an explicit action and a current phone fix inside the effective operating circle and hours. An eligible position follows the phone, including while moving. The server rounds public coordinates and limits publication cadence.

The API evaluates the current hours, session and applicable circle. Closing time, an ineligible position, explicit stop, a settings change that invalidates eligibility, or expiry ends public visibility. An event-only fix expires by the end of its override. Missing updates expire by the server deadline. A later session always starts with a new operator action. The server stores the latest public fix with the session and expires the session record.

## Discovery and menus

The public directory includes approved trucks, even before an operator begins sharing. Consumers can filter by name or cuisine and open each truck’s menu. Only eligible sessions supply coordinates to the public map. Offline profiles include a menu and status when available and omit location.

The operator captures a menu photo and sees its publication result. The API authorizes a signed upload to private S3 and publishes a sanitized JPEG. The consumer sees the capture and publication times with the photo. The directory and active feed use uncached responses.

## Delivery

The API uses the shared Ahara load balancer. The consumer site loads from CloudFront and private S3. Complete and exercise the API and both apps with simulated location before sideloading builds for the field trial.

## API

All timestamps are UTC RFC 3339 and distances are in meters. Operator and admin endpoints accept an Auth0 access token for the Rolling Local audience. Admin commands require the configured owner’s Auth0 subject.

| Request | Behavior |
| --- | --- |
| `GET /health` | Report API availability |
| `GET /public/config` | Return the public sign-in configuration for web, iOS, and Android |
| `GET /public/trucks` | Return truck profiles, menu dates, status, and current eligible map positions |
| `GET /operator` | Return the caller’s registration status, configured profile, and current session |
| `POST /operator/registration` | Submit the caller’s truck for review and return its status on retry |
| `PUT /operator/settings` | Update operating settings on the caller’s approved truck |
| `POST /operator/sessions` | Start sharing with a client-generated session ID and fresh fix |
| `PUT /operator/sessions/{id}/fix` | Submit a fix for the named active session |
| `DELETE /operator/sessions/{id}` | Stop the named session |
| `POST /operator/menu/upload` | Obtain a short-lived signed JPEG upload |
| `POST /operator/menu/publish` | Validate the uploaded image and publish the menu reference |
| `GET /admin/trucks` | List public and private truck records for the owner |
| `POST /admin/trucks` | Add a truck manually and optionally publish it |
| `PUT /admin/trucks/{id}` | Edit listing details, review a pending submission, or link a confirmed operator account |
