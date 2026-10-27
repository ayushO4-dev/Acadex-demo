# Load test results — one API instance, local

> **About this document.** These measurements are from the full Acadex product (one API instance, run locally on 3 October 2026). The load-test scripts and `just load` recipes it mentions aren't part of this demo repository.

Phase 3 of demo-plan.md. Measured 3 October 2026.

**Headline: one `acadex-api` instance (1 vCPU, 512 MiB) sustains 3,000 requests/second on the results-day mix with every read under its p99 budget, and saturates at about 4,500–5,000.** That brackets the ~3,750 rps/instance estimate in [architecture.md §2.3](../architecture.md) and plan.md §11.3, which until now was unmeasured.

## Setup

| | |
|---|---|
| API | Production image (`backend/Dockerfile`, distroless, release build), built with the `load-test-auth` feature so k6 can sign in as students. Limited to **1 vCPU and 512 MiB**, one Cloud Run instance's worth. |
| Cache | Redis 7, per-user summaries, 30 s TTL |
| Database | PostgreSQL 17 in Docker, demo seed: 500 students, ~258k attendance rows, 12k results |
| Load generator | k6 v2.3.0 in Docker on the same Compose network, hitting the API container directly (no web tier, no load balancer) |
| Host | Intel Core Ultra 9 275HX laptop. Docker Desktop VM with **4 CPUs and 4 GB**, shared by k6, Postgres, Redis and the API |
| Users | 499 seeded students, each with a signed token (`just load-setup`) |

The API's sqlx pool is 20 connections, as in production (rule R6).

## The results-day mix

What a student does on results day (`load/lib.js`):

| Share | Request | Notes |
|---|---|---|
| 60% | `GET /students/{id}/results` | 80% latest semester, 20% an earlier one; cached per viewer |
| 20% | `GET /me` | profile, permissions, unread count; not cached |
| 15% | `GET /notices/public` | the public board; `Cache-Control: public`, so the CDN takes it in production |
| 5% | `GET /students/{id}/attendance` | cached per viewer |

Every request carries a JWT that the API verifies (RS256).

## Capacity: fixed rates, 30 s each

`just load-capacity`. A step passes when every endpoint's p99 is under the 150 ms read budget ([architecture.md §12.2](../architecture.md)), errors are under 1%, and k6 dropped no iterations (it could always start a request on time).

| Target | Achieved | Errors | Dropped | p99 results | p99 me | p99 notices | p99 attendance | Results hit ratio | API CPU | |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| 500/s | 500/s | 0% | 0 | 3 ms | 1 ms | 1 ms | 3 ms | 79% | | ✓ |
| 1,000/s | 1,000/s | 0% | 0 | 3 ms | 2 ms | 1 ms | 3 ms | 89% | | ✓ |
| 1,500/s | 1,500/s | 0% | 0 | 4 ms | 3 ms | 2 ms | 3 ms | 92% | | ✓ |
| 2,000/s | 2,000/s | 0% | 0 | 4 ms | 4 ms | 3 ms | 4 ms | 94% | | ✓ |
| **3,000/s** | **3,000/s** | **0%** | **0** | **9 ms** | **13 ms** | **8 ms** | **9 ms** | **96%** | | **✓** |
| 4,000/s | 3,997/s | 0% | 26–97 | 14–24 ms | 20–70 ms | 13–62 ms | 17–27 ms | 97% | 82% | ✗ (drops) |
| 5,000/s | 4,918/s | 0% | 2,388 | 167 ms | 329 ms | 320 ms | 221 ms | 97% | 92% | ✗ |

(4,000/s was run twice; ranges show both runs. CPU is one `docker stats` sample mid-step, as a share of the API's single vCPU.)

**Reading it:**
- **Up to 3,000/s** everything is comfortably inside budget: the highest p99 is 13 ms against 150 ms.
- **At 4,000/s** latencies are still well inside budget, but k6 began dropping iterations. The API was at 82% of its vCPU, so this is the edge of headroom, worsened by k6 competing for the same 4 CPUs.
- **At 5,000/s** the API's vCPU is saturated (92%), queues form, and p99 goes past budget on every endpoint. No request failed: it slows down rather than erroring, the graceful degradation §2.8 asks for.

**The number to quote: 3,000 rps per instance with margin, about 4,000 at the limit.** The architecture's 3,750 estimate sits between them, so the instance floor (3) and ceiling (8+) in §2.3 stand.

## Results-day spike

`just load results-day -e PEAK=300`: from 20/s to 300/s in 15 s, held for a minute, tailing off. 23,300 requests in 100 s.

| Endpoint | p50 | p95 | p99 | max |
|---|---:|---:|---:|---:|
| results | 0.4 ms | 3.2 ms | 5.3 ms | 9.1 ms |
| me | 0.6 ms | 1.0 ms | 1.5 ms | 3.8 ms |
| notices | 0.3 ms | 0.7 ms | 0.9 ms | 2.5 ms |
| attendance | 1.7 ms | 2.6 ms | 3.6 ms | 9.5 ms |

0 errors, 0 dropped. **Results cache hit ratio: 76.6%**, visible in two places: k6's `cache_hit{endpoint:results}` metric (from each response's `x-cache` header), and the API log, which every 10 s prints for example `cache hit ratio, last 10 s cache="results" hits=1451 misses=339 hit_ratio="81.1%"`.

The ratio rises with load: 79% at 500/s, 96% at 3,000/s. A cached entry lives 30 s, so the more often each student asks, the more of the asking is answered from Redis. With only 500 students, each entry is reused just a few times at 300/s; on a real results day, with tens of thousands of students refreshing, the ratio is higher.

## Normal day

`just load normal-day -e RATE=300` for 1 minute: 18,001 requests, 0 errors. p99 per endpoint: me 1.2 ms, attendance 2.5 ms, results 3.7 ms, notices 0.8 ms, payable 1.4 ms. Overall cache hit ratio 77%.

## What this doesn't tell us

- **Cloud Run's CPUs aren't this laptop's.** A Cloud Run vCPU is usually slower than one core of a 2025 laptop chip, so expect a lower number there. Phase 5's cloud load test measures it on the real platform.
- **The database is local and lightly loaded.** At high rates 96% of results are cache hits, so this mostly measures token verification and Redis. The miss path (the database query plus grading) is the p95 of a few milliseconds at low rates, with Cloud SQL network latency still to add.
- **No web tier or load balancer.** Pages through Next.js cost more per request, and that is `acadex-web`'s capacity, not measured here.
- **k6 shares the machine.** At 4,000/s and above, some of the dropped iterations are the load generator's own limits.
- **Warm only.** Cold starts (min instances 0) are measured in Phase 5.

## Reproduce

```bash
just containers                                   # production images
just load-image                                   # the API image that trusts the test key
API_IMAGE=acadex-api:loadtest docker compose --profile containers up -d --no-deps api
just load-setup                                   # test logins and tokens for every student
just load-capacity                                # the capacity table
just load results-day "-e PEAK=300"
just load normal-day "-e RATE=300"
```

Afterwards, put the real API back with `docker compose --profile containers up -d --no-deps api`. The load-test image must never be pushed or deployed.
