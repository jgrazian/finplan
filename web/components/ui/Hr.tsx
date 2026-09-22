export function Hr({ flush }: { flush?: boolean }) {
  return <div className="hr" style={flush ? { margin: 0 } : undefined} />;
}
