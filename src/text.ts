export function cleanDisplayText(source: string) {
  const normalized = source.replace(/\\</g, "<").replace(/\\>/g, ">");
  const request = normalized.match(/<USER_REQUEST[^>]*>([\s\S]*?)(?:<\/USER_REQUEST>|<ADDITIONAL_METADATA[^>]*>|$)/i);
  let visible = request ? request[1] : normalized.replace(/<ADDITIONAL_METADATA[^>]*>[\s\S]*$/i, "");
  visible = visible.replace(/<\/?(?:USER_REQUEST|ADDITIONAL_METADATA|user_instructions|recommended_plugins|environment_context|system-reminder|developer_instructions|app-context|skills_instructions|plugins_instructions)[^>]*>/gi, "");
  return visible.trim();
}
