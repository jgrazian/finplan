"use client";

import { useState } from "react";
import { Button, Dialog, Field, Select } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { ContactTopic } from "@/lib/api/types";

const REPOSITORY_URL = "https://github.com/jgrazian/finplan";

const LINKS = [
  { label: "GitHub", href: REPOSITORY_URL },
  { label: "Documentation", href: `${REPOSITORY_URL}#readme` },
  { label: "Report an issue", href: `${REPOSITORY_URL}/issues/new` },
  { label: "MIT license", href: `${REPOSITORY_URL}/blob/main/LICENSE.txt` },
] as const;

/** Project links and the signed-in user's private contact path. */
export function AppFooter() {
  const [contactOpen, setContactOpen] = useState(false);
  const [topic, setTopic] = useState<ContactTopic>("question");
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [sent, setSent] = useState(false);

  const send = () => {
    setBusy(true);
    setError(undefined);
    api.contact
      .submit({ topic, message })
      .then(() => {
        setMessage("");
        setSent(true);
      })
      .catch((reason: unknown) =>
        setError(reason instanceof Error ? reason.message : "Could not send your message."),
      )
      .finally(() => setBusy(false));
  };

  const openContact = () => {
    setError(undefined);
    setSent(false);
    setContactOpen(true);
  };

  return (
    <>
      <footer className="app-footer">
        <div className="app-footer-summary">
          <strong>FINPLAN</strong>
          <div>
            <span>Open-source retirement planning, built for transparent assumptions.</span>
            <p className="app-footer-note">
              For planning and educational use only — not financial, tax, or investment advice.
            </p>
          </div>
        </div>

        <nav className="app-footer-links" aria-label="Project links">
          {LINKS.map((link) => (
            <a key={link.href} href={link.href} target="_blank" rel="noreferrer">
              {link.label}
              <span aria-hidden="true">↗</span>
            </a>
          ))}
          <button type="button" onClick={openContact}>
            Contact
          </button>
        </nav>
      </footer>

      {contactOpen && (
        <Dialog
          title={sent ? "Message received" : "Contact the FinPlan project"}
          onClose={() => setContactOpen(false)}
          onSubmit={send}
          submitLabel="Send message"
          busy={busy}
          error={error}
          footer={
            sent ? (
              <div style={{ display: "flex", justifyContent: "flex-end" }}>
                <Button type="button" variant="primary" onClick={() => setContactOpen(false)}>
                  Done
                </Button>
              </div>
            ) : undefined
          }
        >
          {sent ? (
            <p className="contact-intro" role="status">
              Thanks for getting in touch. Your message has been saved for the FinPlan team.
            </p>
          ) : (
            <>
              <p className="contact-intro">
                Send a private question or suggestion to the project maintainers.
              </p>

              <Field label="Topic">
                <Select
                  value={topic}
                  onChange={(event) => setTopic(event.target.value as ContactTopic)}
                >
                  <option value="question">Question</option>
                  <option value="feedback">Feedback</option>
                  <option value="bug_report">Bug report</option>
                </Select>
              </Field>

              <Field label="Message">
                <textarea
                  className="input contact-message"
                  value={message}
                  onChange={(event) => setMessage(event.target.value)}
                  placeholder="How can we help?"
                  maxLength={2_000}
                  required
                />
              </Field>

              <p className="contact-privacy">
                Messages are private, but please do not include passwords, account details, or
                personal financial information.
              </p>
            </>
          )}
        </Dialog>
      )}
    </>
  );
}
