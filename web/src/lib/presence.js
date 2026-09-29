/**
 * Presence helpers for concurrent spec editing (HSI §7 Conflict Prevention).
 */

/**
 * Send a UserPresence heartbeat announcing (or clearing) the spec the user is
 * actively editing. The server derives the real user_id from the authenticated
 * connection — the payload user_id is ignored — but the field must be present
 * for the message to deserialize, so a placeholder is sent.
 *
 * @param {{ send: Function, sessionId: string } | null} wsStore
 * @param {object} opts
 * @param {string} opts.workspaceId — workspace scope for the presence entry
 * @param {string|null} [opts.editingEntity] — e.g. "spec:specs/system/auth.md";
 *        omit/null when the user stops editing (banner clears for others)
 * @param {string} [opts.view] — current view label (e.g. "specs")
 */
export function sendEditingPresence(wsStore, { workspaceId, editingEntity = null, view = 'specs' }) {
  if (!wsStore || typeof wsStore.send !== 'function' || !wsStore.sessionId || !workspaceId) {
    return;
  }
  const msg = {
    type: 'UserPresence',
    user_id: 'self', // server overrides with the authenticated identity
    session_id: wsStore.sessionId,
    workspace_id: workspaceId,
    view,
    timestamp: Date.now(),
  };
  // Absent editing_entity => server stores None (user is not editing a spec).
  if (editingEntity) msg.editing_entity = editingEntity;
  wsStore.send(msg);
}

/** Build the presence editing_entity token for a spec path. */
export function specEditingEntity(specPath) {
  return specPath ? `spec:${specPath}` : null;
}
