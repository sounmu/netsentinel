import Link from "next/link";
import { SearchX } from "lucide-react";
import { Panel, EmptyState } from "@/app/components/ui";

export default function NotFound() {
  return (
    <Panel>
      <EmptyState
        icon={<SearchX size={28} aria-hidden="true" />}
        title="Host not found"
        description="The requested host does not exist or is no longer registered."
        action={
          <Link href="/" className="btn btn--primary">
            Back to overview
          </Link>
        }
      />
    </Panel>
  );
}
