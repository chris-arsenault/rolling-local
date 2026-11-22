# MVP operating contract

## Operator enrollment

The Prescott pilot uses managed Ahara accounts with an explicit Rolling Local grant. Operators sign in with the shared Cognito login and create one directory profile on first setup. The signed-in account owns that profile. Set pricing and billing after observing pilot demand and operating costs.

An operator enters their truck name, cuisine, time zone, weekly hours, and a normal operating circle. They can set a separate circle and expiration for an event. Expiry restores the normal circle, and the routine settings are stored independently.

## Location and privacy

The compact widget offers a sharing toggle and a camera shortcut. Starting sharing requires an explicit action and a current phone fix inside the effective operating circle and hours. An eligible position follows the phone, including while moving. The server rounds public coordinates and limits publication cadence.

The API evaluates the current hours, session and applicable circle. Closing time, an ineligible position, explicit stop, a settings change that invalidates eligibility, or expiry ends public visibility. An event-only fix expires by the end of its override. Missing updates expire by the server deadline. A later session always starts with a new operator action. The server stores the latest public fix with the session and expires the session record.

## Discovery and menus

The public directory includes every enrolled truck with a completed profile. Consumers can filter by name or cuisine and open each truck’s menu. Only eligible sessions supply coordinates to the public map. Offline profiles include a menu and status when available and omit location.

The operator captures a menu photo and sees its publication result. The API authorizes a signed upload to private S3, publishes a sanitized JPEG, and stores the capture and publication times. The consumer sees the capture time with the photo. The directory and active feed use uncached responses.

## Delivery

The API uses the shared Ahara load balancer. The consumer site loads from CloudFront and private S3. Complete and exercise the API and both apps with simulated location before sideloading builds for the field trial.

## API

All timestamps are UTC RFC 3339 and distances are in meters. Operator endpoints accept the Rolling Local Cognito access token.

| Request | Behavior |
| --- | --- |
| `GET /health` | Report API availability |
| `GET /public/config` | Give the operator app its public sign-in configuration |
| `GET /public/trucks` | Return truck profiles, menu dates, status, and current eligible map positions |
| `GET /operator` | Return the caller’s profile and current session |
| `PUT /operator/settings` | Create or update the caller’s settings |
| `POST /operator/sessions` | Start sharing with a client-generated session ID and fresh fix |
| `PUT /operator/sessions/{id}/fix` | Submit a fix for the named active session |
| `DELETE /operator/sessions/{id}` | Stop the named session |
| `POST /operator/menu/upload` | Obtain a short-lived signed JPEG upload |
| `POST /operator/menu/publish` | Validate the uploaded image and publish the menu reference |
