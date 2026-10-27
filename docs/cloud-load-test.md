# Cloud load test — Google Cloud, demo profile

> **About this document.** These measurements are from the full Acadex product on Google Cloud (16 October 2026), which this demo repository doesn't deploy. The scripts it mentions under `load/` aren't published here.

Phase 5 of demo-plan.md. Measured 16 October 2026, 13:35–13:50 IST. The local, one-instance capacity numbers are in [load-results.md](load-results.md).

**Headline:**
- **Steady load:** Cloud Run held **500 requests/second for two minutes**, with acadex-api answering in **≤ 28 ms at the 95th percentile** (Cloud Monitoring, server-side).
- **Burst:** a burst to 1,000/s was served at **788/s per minute on 3 instances**, with p95 still 51 ms server-side.
- **Client-side tail:** the slow tail seen by k6 came from the test machine's home connection, not from Google Cloud.
- **Cold start:** a new API instance took **about 10 s** to start. That was Cloud Run's default start-up check, and it now takes **about 1.8 s** (fixed the same day).
- **Memorystore:** the rate limit is now **shared across instances**.

## Setup

| | |
|---|---|
| Target | Cloud Run URLs in `asia-south1`; no load balancer, CDN or Cloud Armor in front (demo profile) |
| acadex-api | Production image, 1 vCPU / 512 MiB, concurrency 80, min 0, max 3, Direct VPC egress to Memorystore |
| acadex-web | One instance exactly (the test payment gateway keeps its state in memory) |
| Cloud SQL | PostgreSQL 17, Enterprise, zonal, 1 vCPU / 3.75 GB, `max_connections` 100 |
| Memorystore | Redis 7.2, Basic, 1 GB, AUTH on (new today, `infra/terraform/memorystore.tf`) |
| Load generator | k6 v2.3.0 in Docker on a laptop in Pune, over a home connection |
| Rate limiting | Off for the two runs (`RATE_LIMITS=off`, approved for the test), then removed. The limiter check ran with it on, and it was re-checked after each run |

**What the requests are:** only real, unauthenticated routes, since the production image doesn't trust the load-test key. Every API request reads Cloud SQL, and none is cached at origin. The signed-in, cached mix was measured per instance locally (`docs/load-results.md`).

Scenario: `load/cloud.js`, a ramping arrival rate of 20 → 200 → 500 (held 2 min) → 1,000 (45 s) → 50 requests/s, over 5½ min.

## Run 2: API only (`MIX=api`)

Two thirds `GET /notices/public` (746 B), one third `GET /institution` (142 B).

| Phase | Asked | Served (client) | Client p50 | Client p95 | Server p50 | Server p95 | API instances |
|---|---:|---:|---:|---:|---:|---:|---:|
| Warm-up | 200/s | 200/s | 28–33 ms | 89–296 ms | 7 ms | 43 ms | 1–2 |
| **Steady** | **500/s** | **500/s** | **35–70 ms** | **133–386 ms** | **10–11 ms** | **24–28 ms** | **3** |
| Burst | 1,000/s | 803–1,005/s | 358–444 ms | 2.3–3.8 s | 12 ms | 51 ms | 3 |

- **Totals:** 146,227 requests at 443/s on average. 0.35% failed, almost all connection timeouts on the client during the burst; 1,970 iterations were dropped.
- **Cloud SQL:** CPU peaked at 60% with 63–64 connections.
- **API CPU:** 95th percentile 58–76%.
- **Client vs server latency:** about 25 ms of the client figure is the round trip from Pune to Mumbai. The rest of the tail, which widened at the burst while server-side latency stayed flat, is the test machine and its connection.
- **Instance count:** reads 4 for the first minutes, because the previous revision was still draining. The new revision stayed within its ceiling of 3.

## Run 1: full page mix

Run 2's API requests at 75%, plus the web app's sign-in page (15%) and a 41 KB icon (10%).

- **Up to 200/s:** everything was quick from Pune (p50 about 30 ms, p95 under 75 ms).
- **From about 300/s:**
  - The one web instance saturated. Its pages took 9–16 s, and Cloud Run refused 219 of them with 429, having no instance free.
  - Its bytes (2.9 MB/s on average) filled the home connection, so the API's small replies queued on the client too, at p50 0.7–1.5 s, while the server answered in about 10 ms.
- **Totals:** 105,298 requests at 319/s, 0.96% failed, and 42,884 iterations dropped.

This is expected of the demo shape: the web app is pinned to one instance for the test gateway. In production, with Razorpay, it scales like the API, and the CDN serves the sign-in page's assets.

## Memorystore: the rate limit is shared

The check was `load/limiter.js`:
- acadex-api was held at 3 warm instances, each taking 5 concurrent requests, so the burst spread over all three.
- One address sent 300 reads at once.
- Each caller may make 100 reads a minute.

| Instance | Let through | Refused (429) |
|---|---:|---:|
| 1 (…7cdc) | 29 | 78 |
| 2 (…48a4) | 32 | 58 |
| 3 (…41f0) | 40 | 63 |
| **Total** | **101** | **199** |

Buckets counted per instance (the in-process fallback) would have let up to 300 through. The same burst after each run, with rate limiting restored: 101 let through, 199 refused.

## What to do about it

| Finding | Action |
|---|---|
| Cold start: median 10.0 s, p99 10.5 s for acadex-api | **Fixed the same day (0dd56da):** the startup probe used Cloud Run's default 10 s period, so an instance ready in about a second waited for the second check. It now checks every second, and a cold start takes **about 1.8 s** (instance start to probe passed). That was also behind the occasional "API unreachable" page, since the web app gave up after 5 s (now 12 s). Min instances 1 on demo day is now optional |
| The web app is one instance, so it saturates above about 100 page requests a second | Fine for the demo. Production uses Razorpay, so web scales |
| Public reads hit Cloud SQL every time: about 260/s per instance at 60–76% CPU, against 3,000/s per instance locally for cached, signed-in reads | By design. In production the CDN serves `/notices/public` (`Cache-Control: public`) |
| Cloud SQL connections peaked at 64 of 100; each API instance opens up to 20 | Watch it: a 4th instance plus the worker and jobs would come close. Raise `max_connections` or lower the pool before raising the API's ceiling |
| The limiter keyed anonymous callers on the first `X-Forwarded-For` entry, which a client can set | **Fixed (30a19da):** only the address Cloud Run appends counts, or one the web app forwards with a shared key. Verified live: 300 reads with forged headers shared one bucket |

## Reproduce

```bash
# rate limiting off for the run (it limits one IP to 100 reads a minute)
gcloud run services update acadex-api --region asia-south1 --update-env-vars RATE_LIMITS=off
docker run --rm -v ./load:/scripts grafana/k6 run -e MIX=api \
  --summary-export /scripts/results/cloud2-summary.json --out csv=/scripts/results/cloud2-requests.csv /scripts/cloud.js
gcloud run services update acadex-api --region asia-south1 --remove-env-vars RATE_LIMITS
python3 load/cloud_summarise.py load/results/cloud2-requests.csv
python3 load/cloud_metrics.py <start> <end>    # Cloud Monitoring series for the window
```
