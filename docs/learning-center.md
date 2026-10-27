# Learning Center, metric tooltips and the guided tour

Three connected pieces in `web/frontend`:

| Piece | Where |
| --- | --- |
| Lessons (content as data) | `lib/learn/content/*.ts`, model in `lib/learn/types.ts`, index/search in `lib/learn/index.ts` and `lib/learn/search.ts` |
| Learning Center pages | `app/learn/page.tsx`, `app/learn/[category]/page.tsx`, `app/learn/[category]/[slug]/page.tsx`; components in `components/learn/` |
| Metric registry (tooltip text + lesson link) | `lib/learn/metrics.ts` |
| ⓘ tooltip | `components/ui/MetricInfo.tsx` (`InfoTip`, `MetricLabel`) |
| Guided tour | `components/onboarding/` (driver.js), steps in `tourSteps.ts` |
| Tour state | `user_onboarding` table (`db/migrations/012_onboarding.sql`), `lib/server/onboardingStore.ts`, `GET/PUT /api/auth/onboarding`, shared logic in `lib/onboarding.ts` |

## Adding a lesson

Add an entry to the right file in `lib/learn/content/`. Metric lessons use `metricLesson()` (in
`lib/learn/build.ts`), which requires every part: explanation, why it matters, formula, how the engine
computes it, worked example, high/low interpretation, misconceptions and related metrics. Inline
markup: `**bold**`, `` `code` ``, `\(tex\)` and `[label](/path)`. Formulas are KaTeX, rendered on
the server.

Formulas and worked examples must match the engine. `lib/learn/calc.ts` ports the engine's metric
code; `scripts/test-learn.mjs` checks it against the engine's own unit-test values and against a
real run (`scripts/fixtures/learn-engine-run.json`), and checks that the numbers stated in the main
worked examples follow from the formulas.

## Adding a metric tooltip

1. Add the metric to `metricDefinitions` in `lib/learn/metrics.ts`, with `learnPath` pointing at its lesson.
2. Use `<MetricLabel metric="id">Label</MetricLabel>` or `<InfoTip metric="id" />` next to the value.

`npm run test:learn` fails if a `learnPath` does not resolve, if a metric lesson has no registry
entry, or if a component uses an unknown id.

## The tour

It starts by itself once per user and tour version, after the Studio has rendered, and never again
once the user finishes or dismisses it (state is per user in Postgres, with a per-user localStorage copy
used only when the API cannot be reached). Help → Take the tour, Help → Tour this page, and the Learning
Center's "Take the website tour" (`/studio?tour=1`) restart it. Steps target `data-tour="..."`
attributes and fall back to a centred card when a target is hidden (for example sidebar items on
phones). The tour only highlights; highlighted elements cannot be clicked during a step and it makes
no request other than saving its own progress.

Bump `TOUR_VERSION` in `lib/onboarding.ts` to show a substantially changed tour once more to
everyone.
