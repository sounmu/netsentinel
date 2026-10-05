"use client";

import { useEffect } from "react";
import { AlertTriangle } from "lucide-react";
import { Button, Panel, EmptyState } from "@/app/components/ui";

export default function ErrorPage({
  error,
  reset,
}: {
  error: Error & { digest?: string };
  reset: () => void;
}) {
  useEffect(() => {
    console.error(error);
  }, [error]);

  return (
    <Panel>
      <EmptyState
        tone="error"
        icon={<AlertTriangle size={28} aria-hidden="true" />}
        title="Something went wrong"
        description="The dashboard hit an unexpected error while rendering this route."
        action={
          <Button variant="primary" onClick={reset}>
            Try again
          </Button>
        }
      />
    </Panel>
  );
}
