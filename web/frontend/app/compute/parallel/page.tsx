export default function ParallelPage() {
  return (
    <div className="p-6">
      <h1 className="text-2xl font-bold">Parallel Backend</h1>
      <p className="text-foreground-muted mt-2">
        Multi-threaded CPU backend. Paths are simulated in fixed chunks with their own seeds, so results depend only on
        the seed and path count, never on the number of threads.
      </p>
    </div>
  );
}
