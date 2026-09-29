#!/usr/bin/env python3
"""The App Store Connect steps the build does by itself, with the upload key.

    asc.py prepare BUNDLE_ID   register the app's IDs (the app's and its
                               broadcast's), and check the app exists in
                               App Store Connect
    asc.py testers BUNDLE_ID   make sure the account holder gets every build
                               in TestFlight (an internal group with access
                               to all builds)

Reads ASC_KEY_ID, ASC_ISSUER_ID and ASC_KEY_PATH (the .p8 file) from the
environment. Speaks in GitHub Actions annotations. Never prints the key or
anyone's email address.
"""

import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

import jwt  # PyJWT, with cryptography for ES256

API = "https://api.appstoreconnect.apple.com/v1"
GROUP = "Syrup"


def token() -> str:
    with open(os.environ["ASC_KEY_PATH"]) as f:
        key = f.read()
    now = int(time.time())
    claims = {"iss": os.environ["ASC_ISSUER_ID"], "iat": now, "exp": now + 900, "aud": "appstoreconnect-v1"}
    return jwt.encode(claims, key, algorithm="ES256", headers={"kid": os.environ["ASC_KEY_ID"], "typ": "JWT"})


TOKEN = ""


def call(method: str, path: str, query: dict | None = None, body: dict | None = None) -> tuple[int, dict]:
    url = API + path + ("?" + urllib.parse.urlencode(query) if query else "")
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(
        url, data=data, method=method, headers={"Authorization": "Bearer " + TOKEN, "Content-Type": "application/json"}
    )
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            raw = r.read()
            return r.status, json.loads(raw) if raw else {}
    except urllib.error.HTTPError as e:
        raw = e.read()
        try:
            return e.code, json.loads(raw)
        except ValueError:
            return e.code, {"errors": [{"detail": raw.decode(errors="replace")[:300]}]}
    except (urllib.error.URLError, TimeoutError, OSError) as e:
        return 0, {"errors": [{"detail": f"couldn't reach App Store Connect: {e}"}]}


def why(answer: dict) -> str:
    return "; ".join((e.get("detail") or e.get("title") or "") for e in answer.get("errors", []))[:500]


def say(level: str, title: str, message: str) -> None:
    message = message.replace("%", "%25").replace("\n", "%0A")
    print(f"::{level} title={title}::{message}", flush=True)


def bundle_id(identifier: str, name: str) -> bool:
    """Registers `identifier` unless it is. True if it was just registered."""
    status, answer = call("GET", "/bundleIds", {"filter[identifier]": identifier, "limit": "200"})
    if status != 200:
        say("error", "App ID", f"couldn't list the app IDs ({status}): {why(answer)}")
        sys.exit(1)
    if any(b["attributes"]["identifier"] == identifier for b in answer.get("data", [])):
        return False
    body = {"data": {"type": "bundleIds", "attributes": {"identifier": identifier, "name": name, "platform": "IOS"}}}
    status, answer = call("POST", "/bundleIds", body=body)
    if status not in (200, 201):
        say("error", "App ID", f"couldn't register {identifier} ({status}): {why(answer)}")
        sys.exit(1)
    return True


def app_for(bundle: str) -> str | None:
    status, answer = call("GET", "/apps", {"filter[bundleId]": bundle, "limit": "10"})
    if status != 200:
        say("error", "App Store Connect", f"couldn't look the app up ({status}): {why(answer)}")
        sys.exit(1)
    for a in answer.get("data", []):
        if a["attributes"].get("bundleId") == bundle:
            return a["id"]
    return None


def prepare(bundle: str) -> None:
    made = [b for b, n in ((bundle, "Syrup"), (bundle + ".eyes", "Syrup Eyes")) if bundle_id(b, n)]
    if made:
        say("notice", "App ID", "registered " + ", ".join(made))
    if app_for(bundle) is None:
        say(
            "error",
            "App Store Connect",
            f"There's no app for {bundle} yet, and Apple only lets a person create one. Once: "
            "https://appstoreconnect.apple.com/apps, then +, New App: iOS, a name that's free "
            f"(e.g. Syrup Game Coach), English, the bundle ID {bundle}, SKU syrup. "
            "Then run this workflow again (Actions, iPhone app, Re-run all jobs).",
        )
        sys.exit(1)
    print("the app exists in App Store Connect")


def testers(bundle: str) -> None:
    app = app_for(bundle)
    if app is None:
        return
    manual = "Add yourself once: App Store Connect, the app, TestFlight, Internal Testing, +."
    status, answer = call("GET", f"/apps/{app}/betaGroups", {"limit": "200"})
    group = next(
        (g for g in answer.get("data", []) if g["attributes"].get("isInternalGroup") and g["attributes"].get("name") == GROUP),
        None,
    )
    if group is None:
        body = {
            "data": {
                "type": "betaGroups",
                "attributes": {"name": GROUP, "isInternalGroup": True, "hasAccessToAllBuilds": True},
                "relationships": {"app": {"data": {"type": "apps", "id": app}}},
            }
        }
        status, answer = call("POST", "/betaGroups", body=body)
        if status not in (200, 201):
            say("warning", "TestFlight", f"couldn't make a testing group ({status}): {why(answer)}. {manual}")
            return
        group = answer["data"]
    status, answer = call("GET", "/users", {"limit": "200"})
    owner = next((u for u in answer.get("data", []) if "ACCOUNT_HOLDER" in u["attributes"].get("roles", [])), None)
    if owner is None:
        say("warning", "TestFlight", f"couldn't find the account holder ({status}). {manual}")
        return
    email = owner["attributes"].get("username") or owner["attributes"].get("email") or ""
    status, answer = call("GET", f"/betaGroups/{group['id']}/betaTesters", {"limit": "200"})
    if any((t["attributes"].get("email") or "").lower() == email.lower() for t in answer.get("data", [])):
        print("the account holder is already a tester")
        return
    person = {k: owner["attributes"][k] for k in ("firstName", "lastName") if owner["attributes"].get(k)}
    body = {
        "data": {
            "type": "betaTesters",
            "attributes": {"email": email, **person},
            "relationships": {"betaGroups": {"data": [{"type": "betaGroups", "id": group["id"]}]}},
        }
    }
    status, answer = call("POST", "/betaTesters", body=body)
    if status in (200, 201):
        say("notice", "TestFlight", "You're a tester: Apple emails an invitation, and every build appears in the TestFlight app.")
        return
    # Already a tester of this team: add them to the group.
    status2, found = call("GET", "/betaTesters", {"filter[email]": email, "limit": "5"})
    tester = next(iter(found.get("data", [])), None) if status2 == 200 else None
    if tester is not None:
        link = {"data": [{"type": "betaTesters", "id": tester["id"]}]}
        status3, answer3 = call("POST", f"/betaGroups/{group['id']}/relationships/betaTesters", body=link)
        if status3 in (200, 201, 204):
            say("notice", "TestFlight", "You're in the Syrup testing group: every build appears in the TestFlight app.")
            return
        answer = answer3
    say("warning", "TestFlight", f"couldn't add you as a tester ({status}): {why(answer)}. {manual}")


def main() -> None:
    global TOKEN
    if len(sys.argv) != 3 or sys.argv[1] not in ("prepare", "testers"):
        print(__doc__)
        sys.exit(2)
    TOKEN = token()
    {"prepare": prepare, "testers": testers}[sys.argv[1]](sys.argv[2])


if __name__ == "__main__":
    main()
