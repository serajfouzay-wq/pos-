/** Inline error from a failed query or mutation. */
export function ErrorText({ error }: { error: Error | null | undefined }) {
  if (!error) return null;
  return (
    <p role="alert" className="error">
      {error.message}
    </p>
  );
}
