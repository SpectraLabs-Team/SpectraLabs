export default function CUDAPage() {
  return (
    <div className="p-6">
      <h1 className="text-2xl font-bold">CUDA Backend</h1>
      <p className="text-foreground-muted mt-2">
        Not included in this build. Runs that request CUDA fall back to the Parallel backend, and benchmarks report them
        as a fallback rather than as GPU results.
      </p>
    </div>
  );
}
