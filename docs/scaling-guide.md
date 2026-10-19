# M20b Step 13: Scaling Guide

## 1. S3 Artifact Storage

Move from local volume to S3 (or MinIO for self-hosted).

### Setup

**AWS S3:**
```bash
# Create bucket
aws s3api create-bucket \
  --bucket spectra-artifacts-prod \
  --region us-west-2 \
  --create-bucket-configuration LocationConstraint=us-west-2

# Create IAM user for app
aws iam create-user --user-name spectra-app

# Attach policy
aws iam attach-user-policy \
  --user-name spectra-app \
  --policy-arn arn:aws:iam::aws:policy/AmazonS3FullAccess

# Get credentials
aws iam create-access-key --user-name spectra-app
```

**MinIO (self-hosted):**
```bash
# Start MinIO
docker run -d \
  -p 9000:9000 \
  -p 9001:9001 \
  -e MINIO_ROOT_USER=minioadmin \
  -e MINIO_ROOT_PASSWORD=minioadmin \
  minio/minio server /data --console-address ":9001"

# Create bucket
mc mb minio/spectra-artifacts

# Create service account
mc admin user svcacct add minio minioadmin
```

### Code Changes

Update `crates/spectra-db/src/artifacts.rs`:

```rust
// Add feature flag for S3
#[cfg(feature = "s3")]
pub struct S3ArtifactStore {
    client: aws_sdk_s3::Client,
    bucket: String,
}

#[cfg(feature = "s3")]
impl S3ArtifactStore {
    pub async fn put(&self, bytes: &[u8]) -> Result<Stored> {
        let sha256 = sha256_hex(bytes);
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&Self::relative_path(&sha256))
            .body(bytes.into())
            .send()
            .await?;
        Ok(Stored { sha256, path: Self::relative_path(&sha256), size: bytes.len() as u64 })
    }
}
```

### Environment Variables

```bash
# In .env
ARTIFACT_STORAGE=s3  # or 'local'
S3_BUCKET=spectra-artifacts-prod
S3_REGION=us-west-2
AWS_ACCESS_KEY_ID=...
AWS_SECRET_ACCESS_KEY=...
# OR for MinIO:
S3_ENDPOINT=http://minio:9000
```

## 2. Multiple Workers

Scale the worker horizontally.

### Docker Compose

```bash
# Start 3 worker instances
docker compose up --scale worker=3 -d

# Monitor
docker compose logs -f worker
```

### Kubernetes

```yaml
# worker-deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: spectra-worker
spec:
  replicas: 3  # scale by changing this
  selector:
    matchLabels:
      app: spectra-worker
  template:
    metadata:
      labels:
        app: spectra-worker
    spec:
      containers:
      - name: worker
        image: spectra-labs/engine:latest
        command: ["spectra-worker"]
        env:
        - name: DATABASE_URL
          valueFrom:
            secretKeyRef:
              name: spectra-secrets
              key: database-url
        resources:
          requests:
            cpu: "2"
            memory: "2Gi"
          limits:
            cpu: "4"
            memory: "4Gi"
```

### Job Queue Split by Kind (optional)

Run different workers for different job kinds:

```bash
# Worker 1: risk jobs only
SPECTRA_WORKER_KINDS=risk docker compose up -d worker-risk

# Worker 2: benchmark jobs only
SPECTRA_WORKER_KINDS=benchmark docker compose up -d worker-benchmark

# Worker 3: stress jobs only
SPECTRA_WORKER_KINDS=stress docker compose up -d worker-stress
```

## 3. TypeScript Client Generation

Generate auto-typed client from OpenAPI spec.

### Generate

```bash
./scripts/generate-typescript-client.sh http://api.example.com/api/v1/openapi.json

# Or use OpenAPI Generator CLI directly
openapi-generator-cli generate \
  -i http://localhost:8000/api/v1/openapi.json \
  -g typescript-fetch \
  -o ./web/frontend/src/lib/api-client
```

### Use in Frontend

```typescript
// Before (hand-written)
const response = await fetch('/api/v1/jobs', {
  method: 'POST',
  body: JSON.stringify({ kind: 'risk', payload: {...} }),
});
const data = await response.json();

// After (generated client - fully typed)
import { DefaultApi } from './lib/api-client';

const api = new DefaultApi();
const job = await api.createJob({
  kind: 'risk',
  payload: {...},
  priority: 0,
});
// job is fully typed with CreateJobRequest, JobView, etc.
```

## 4. Plugin Interface

Version the RiskPlugin trait and extend to support other plugins.

### Current (M20a)

```rust
pub trait RiskPlugin {
    fn compute_risk(&self, portfolio: &Portfolio, ...) -> RiskMetrics;
}
```

### Versioned (M20b)

```rust
pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn capabilities(&self) -> Vec<Capability>;
}

pub trait RiskPlugin: Plugin {
    fn compute_risk(&self, ...) -> RiskMetrics;
}

pub trait BacktestPlugin: Plugin {
    fn run_backtest(&self, ...) -> BacktestResult;
}

// Allow plugins to be registered at runtime
pub struct PluginRegistry {
    plugins: HashMap<String, Box<dyn Plugin>>,
}

impl PluginRegistry {
    pub fn register<P: Plugin + 'static>(&mut self, plugin: P) {
        self.plugins.insert(plugin.name().to_string(), Box::new(plugin));
    }

    pub fn get_risk_plugin(&self, name: &str) -> Option<&dyn RiskPlugin> {
        self.plugins.get(name).and_then(|p| {
            p.as_any().downcast_ref::<dyn RiskPlugin>()
        })
    }
}
```

## Scaling Checklist

- [ ] Switch artifact storage to S3/MinIO
- [ ] Deploy 3+ worker instances
- [ ] Set up job queue split by kind (optional)
- [ ] Generate TypeScript client from OpenAPI
- [ ] Integrate generated client in frontend
- [ ] Version RiskPlugin and create PluginRegistry
- [ ] Add support for other plugin types (backtest, optimization, etc.)
- [ ] Monitor: queue depth, worker utilization, job duration
- [ ] Add alerting: queue depth > threshold, worker down, artifact quota exceeded

## Performance Targets

With scaling:
- **Queue throughput:** 100+ jobs/hour
- **Concurrent jobs:** 8+ per worker, 50+ total
- **Job latency:** <5s queue time, <10s median duration
- **Artifact storage:** unlimited (S3 scales automatically)
- **API response time:** <500ms p95 (with caching)

## Cost Optimization

- **S3:** ~$0.023/GB/month storage, ~$0.0004/1000 requests
- **EC2 workers:** t3.medium ($0.04/hour) = ~$30/month per worker
- **Database:** managed RDS cheaper than self-hosted at scale

---

**M20a + M20b complete.** All three steps of M20b (Observability, Deployment, Scaling) are configured and ready.
