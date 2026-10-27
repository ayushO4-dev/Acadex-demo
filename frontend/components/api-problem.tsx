import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { ApiError } from "@/lib/api";

/** An API error as shown to the user, with the trace id support will ask for. */
export function ApiProblem({ error }: { error: unknown }) {
  const problem = error instanceof ApiError ? error.problem : null;
  return (
    <Alert variant="destructive">
      <AlertTitle>{problem?.title ?? "Acadex couldn't reach its server"}</AlertTitle>
      <AlertDescription>
        {problem?.detail ??
          (process.env.NODE_ENV === "development"
            ? "Check that the API is running (just dev)."
            : "Reload the page in a moment. If it keeps happening, tell the support desk.")}
        {problem?.trace_id && <span className="block font-mono text-xs">trace {problem.trace_id}</span>}
      </AlertDescription>
    </Alert>
  );
}
