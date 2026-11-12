use cairn_core::diff::FileDiff;
use cairn_core::stack::{CheckRun, PrComment, PrCommit, PrDetails, PrFile, PrReviewer, PullRequest, ReviewComment};

/// The signed-in user in the mock data.
pub(crate) const ME: &str = "mira";

/// One fake pull request with everything the page shows.
pub(crate) struct MockPr {
    pub details: PrDetails,
    pub diffs: Vec<FileDiff>,
    pub inline: Vec<ReviewComment>,
}

fn check(name: &str, status: &str, conclusion: &str) -> CheckRun {
    CheckRun { name: name.into(), status: status.into(), conclusion: conclusion.into() }
}

fn comment(author: &str, created: &str, body: &str, review: Option<&str>) -> PrComment {
    PrComment { author: author.into(), created: created.into(), body: body.into(), review_state: review.map(str::to_string) }
}

fn commit(oid: &str, headline: &str, author: &str, date: &str) -> PrCommit {
    PrCommit { oid: oid.into(), headline: headline.into(), author: author.into(), date: date.into() }
}

fn inline(author: &str, path: &str, line: u64, body: &str) -> ReviewComment {
    ReviewComment { author: author.into(), path: path.into(), line: Some(line), body: body.into() }
}

fn reviewer(login: &str, state: &str) -> PrReviewer {
    PrReviewer { login: login.into(), state: state.into() }
}

const OFFLINE_DIFF: &str = "diff --git a/src/sync/queue.ts b/src/sync/queue.ts
new file mode 100644
--- /dev/null
+++ b/src/sync/queue.ts
@@ -0,0 +1,24 @@
+import { Storage } from '../storage';
+
+export interface PendingChange {
+  id: string;
+  kind: 'create' | 'update' | 'delete';
+  payload: unknown;
+  createdAt: number;
+}
+
+export class SyncQueue {
+  constructor(private storage: Storage) {}
+
+  async push(change: PendingChange) {
+    const all = await this.storage.get<PendingChange[]>('queue', []);
+    all.push(change);
+    await this.storage.set('queue', all);
+  }
+
+  async drain(send: (c: PendingChange) => Promise<void>) {
+    const all = await this.storage.get<PendingChange[]>('queue', []);
+    for (const c of all) await send(c);
+    await this.storage.set('queue', []);
+  }
+}
diff --git a/src/api/client.ts b/src/api/client.ts
--- a/src/api/client.ts
+++ b/src/api/client.ts
@@ -12,14 +12,22 @@ export class ApiClient {
   constructor(private baseUrl: string, private token: () => string) {}

   async request<T>(path: string, init: RequestInit = {}): Promise<T> {
-    const res = await fetch(this.baseUrl + path, {
-      ...init,
-      headers: { Authorization: `Bearer ${this.token()}`, ...init.headers },
-    });
+    if (!navigator.onLine) {
+      throw new OfflineError(path);
+    }
+    const res = await fetch(this.baseUrl + path, {
+      ...init,
+      headers: { Authorization: `Bearer ${this.token()}`, ...init.headers },
+      signal: AbortSignal.timeout(15_000),
+    });
     if (!res.ok) {
       throw new ApiError(res.status, await res.text());
     }
     return res.json() as Promise<T>;
   }
 }
+
+export class OfflineError extends Error {
+  constructor(public path: string) { super(`offline: ${path}`); }
+}
diff --git a/src/screens/Home.tsx b/src/screens/Home.tsx
--- a/src/screens/Home.tsx
+++ b/src/screens/Home.tsx
@@ -3,9 +3,15 @@ import { useTasks } from '../hooks/useTasks';

 export function Home() {
   const { tasks, loading } = useTasks();
+  const online = useOnline();

   return (
     <Screen title=\"Today\">
+      {!online && (
+        <Banner tone=\"warning\">
+          You're offline. Changes are saved and sent when you're back.
+        </Banner>
+      )}
       {loading ? <Spinner /> : <TaskList tasks={tasks} />}
     </Screen>
   );
diff --git a/README.md b/README.md
--- a/README.md
+++ b/README.md
@@ -20,6 +20,10 @@ npm run ios

 ## Features

+### Offline mode
+
+Tasks you add or change without a connection are kept in a queue and sent in order once you are back online.
+
 - Daily task list
 - Reminders
 - Shared lists
";

const TOKEN_DIFF: &str = "diff --git a/src/auth/session.ts b/src/auth/session.ts
--- a/src/auth/session.ts
+++ b/src/auth/session.ts
@@ -30,10 +30,16 @@ export class Session {
   async token(): Promise<string> {
-    if (Date.now() > this.expiresAt) {
-      return this.current.token;
-    }
+    if (Date.now() > this.expiresAt - 30_000) {
+      try {
+        await this.refresh();
+      } catch {
+        this.signOut();
+        throw new SessionExpiredError();
+      }
+    }
     return this.current.token;
   }
";

const SETTINGS_DIFF: &str = "diff --git a/src/screens/Settings.tsx b/src/screens/Settings.tsx
--- a/src/screens/Settings.tsx
+++ b/src/screens/Settings.tsx
@@ -1,8 +1,12 @@
-import { List, Row } from '../ui';
+import { Card, Section, Toggle } from '../ui';

 export function Settings() {
   return (
-    <List>
-      <Row title=\"Notifications\" />
-    </List>
+    <Section title=\"General\">
+      <Card>
+        <Toggle label=\"Notifications\" />
+        <Toggle label=\"Sounds\" />
+      </Card>
+    </Section>
   );
 }
";

const BUMP_DIFF: &str = "diff --git a/package.json b/package.json
--- a/package.json
+++ b/package.json
@@ -14,7 +14,7 @@
   \"dependencies\": {
     \"react\": \"19.1.0\",
-    \"date-fns\": \"4.1.0\",
+    \"date-fns\": \"4.2.0\",
     \"zod\": \"3.25.0\"
   }
 }
";

#[allow(clippy::too_many_arguments)]
fn pr(number: u64, title: &str, author: &str, head: &str, created: &str, draft: bool, review: &str, checks: Vec<CheckRun>, labels: &[&str], requests: &[&str]) -> PullRequest {
    PullRequest {
        number,
        title: title.into(),
        url: format!("https://github.com/aurora-labs/aurora-mobile/pull/{number}"),
        state: "OPEN".into(),
        is_draft: draft,
        review: review.into(),
        base: "main".into(),
        head: head.into(),
        checks,
        comments: Vec::new(),
        author: author.into(),
        created: created.into(),
        additions: 0,
        deletions: 0,
        changed_files: 0,
        labels: labels.iter().map(|s| s.to_string()).collect(),
        review_requests: requests.iter().map(|s| s.to_string()).collect(),
    }
}

fn finish(mut details: PrDetails, diff: &str, inline: Vec<ReviewComment>) -> MockPr {
    let diffs = cairn_core::diff::parse_diff(diff);
    details.files = diffs.iter().map(|d| {
        let (a, r) = d.stats();
        PrFile { path: d.path.clone(), additions: a, deletions: r }
    }).collect();
    details.pr.additions = details.files.iter().map(|f| f.additions).sum();
    details.pr.deletions = details.files.iter().map(|f| f.deletions).sum();
    details.pr.changed_files = details.files.len() as u32;
    MockPr { details, diffs, inline }
}

/// Builds the fake pull requests shown when `CAIRN_DEBUG_MOCK_PRS` is set.
///
/// # Returns
/// Four pull requests in different states: waiting for your review, ready to merge, draft, failing.
pub(crate) fn mock_prs() -> Vec<MockPr> {
    let offline = PrDetails {
        pr: pr(
            48,
            "Add offline mode to the mobile app",
            "leo",
            "feat/offline-mode",
            "2026-11-10T09:12:00Z",
            false,
            "REVIEW_REQUIRED",
            vec![check("build", "completed", "success"), check("unit tests", "completed", "success"), check("lint", "completed", "success"), check("e2e (iOS)", "in_progress", ""), check("preview deploy", "queued", "")],
            &["feature", "mobile"],
            &[ME],
        ),
        author: "leo".into(),
        body: "Lets people keep working without a connection.\n\n- Changes made offline go into a queue and are sent in order when the app is back online.\n- The home screen shows a banner while offline.\n- API calls fail fast with an OfflineError instead of hanging.\n\nTested on a plane, which was a good excuse to fly somewhere.".into(),
        head_oid: "9f2c1e7a".into(),
        conversation: vec![
            comment("ann", "2026-11-10T14:03:00Z", "Love this. One question: what happens if the same task is edited twice while offline? Do we send both updates?", None),
            comment("leo", "2026-11-10T15:20:00Z", "Both go out in order, the server keeps the last one. I'll squash them client-side in a follow-up.", None),
            comment("ann", "2026-11-11T08:45:00Z", "Works for me, nice work.", Some("APPROVED")),
        ],
        files: Vec::new(),
        commits: vec![
            commit("4b1d2e0a77", "Add a persistent sync queue", "leo", "2026-11-10T09:01:00Z"),
            commit("8c3f9a1b21", "Fail fast when offline", "leo", "2026-11-10T09:10:00Z"),
            commit("2a7e4c9d05", "Show an offline banner on Home", "leo", "2026-11-10T16:30:00Z"),
            commit("9f2c1e7a63", "Document offline mode", "leo", "2026-11-11T10:02:00Z"),
        ],
        reviewers: vec![reviewer("ann", "APPROVED"), reviewer(ME, "PENDING")],
        assignees: vec!["leo".into()],
        merge_state: "BLOCKED".into(),
    };
    let offline_inline = vec![
        inline("ann", "src/sync/queue.ts", 22, "If `send` throws halfway, the items already sent are sent again next time. Can we remove them one by one?"),
        inline("ann", "src/api/client.ts", 22, "15 seconds feels long on a phone, maybe 8?"),
    ];

    let token = PrDetails {
        pr: pr(
            47,
            "Fix crash when the session token expires",
            ME,
            "fix/token-expiry",
            "2026-11-09T11:30:00Z",
            false,
            "APPROVED",
            vec![check("build", "completed", "success"), check("unit tests", "completed", "success"), check("lint", "completed", "success")],
            &["bug"],
            &[],
        ),
        author: ME.into(),
        body: "Refreshes the token 30 seconds before it runs out, and signs out cleanly when the refresh fails instead of crashing.\n\nFixes #31.".into(),
        head_oid: "c0ffee12".into(),
        conversation: vec![comment("leo", "2026-11-09T13:10:00Z", "Clean fix, thanks!", Some("APPROVED"))],
        files: Vec::new(),
        commits: vec![commit("c0ffee1234", "Refresh the token before it expires", ME, "2026-11-09T11:25:00Z")],
        reviewers: vec![reviewer("leo", "APPROVED")],
        assignees: vec![ME.into()],
        merge_state: "CLEAN".into(),
    };

    let settings = PrDetails {
        pr: pr(45, "Redesign the settings screen", "ann", "design/settings", "2026-11-08T10:00:00Z", true, "", Vec::new(), &["design"], &[]),
        author: "ann".into(),
        body: "Work in progress: moving settings into cards with sections.".into(),
        head_oid: "abc12345".into(),
        conversation: Vec::new(),
        files: Vec::new(),
        commits: vec![commit("abc1234567", "Group settings into sections", "ann", "2026-11-08T09:55:00Z")],
        reviewers: Vec::new(),
        assignees: vec!["ann".into()],
        merge_state: "DRAFT".into(),
    };

    let bump = PrDetails {
        pr: pr(
            44,
            "Bump date-fns from 4.1.0 to 4.2.0",
            "dependabot",
            "dependabot/npm/date-fns-4.2.0",
            "2026-11-06T06:00:00Z",
            false,
            "REVIEW_REQUIRED",
            vec![check("build", "completed", "success"), check("unit tests", "completed", "failure"), check("lint", "completed", "success")],
            &["dependencies"],
            &[ME],
        ),
        author: "dependabot".into(),
        body: "Bumps date-fns from 4.1.0 to 4.2.0.\n\nRelease notes: formatting of relative dates now respects the locale's week start.".into(),
        head_oid: "d3adb33f".into(),
        conversation: Vec::new(),
        files: Vec::new(),
        commits: vec![commit("d3adb33f00", "Bump date-fns from 4.1.0 to 4.2.0", "dependabot", "2026-11-06T06:00:00Z")],
        reviewers: vec![reviewer(ME, "PENDING")],
        assignees: Vec::new(),
        merge_state: "BEHIND".into(),
    };

    vec![
        finish(offline, OFFLINE_DIFF, offline_inline),
        finish(token, TOKEN_DIFF, Vec::new()),
        finish(settings, SETTINGS_DIFF, Vec::new()),
        finish(bump, BUMP_DIFF, Vec::new()),
    ]
}
