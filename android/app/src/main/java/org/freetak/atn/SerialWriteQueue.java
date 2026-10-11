package org.freetak.atn;

import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.List;

/** Pure scheduler: a callback completes one write; failures cancel the remaining transfer. */
final class SerialWriteQueue {
    static final class Write {
        final String operationId;
        final int frameNumber;
        final String sessionId;
        final String connectionId;
        final byte[] bytes;
        Write(String operationId, byte[] bytes) { this(operationId, 1, "", "", bytes); }
        Write(String operationId, int frameNumber, String sessionId, String connectionId, byte[] bytes) {
            this.operationId = operationId; this.frameNumber = frameNumber;
            this.sessionId = sessionId; this.connectionId = connectionId; this.bytes = bytes.clone();
        }
    }
    private final int limit;
    private final ArrayDeque<Write> pending = new ArrayDeque<>();
    private Write active;
    SerialWriteQueue(int limit) {
        if (limit < 1) throw new IllegalArgumentException("Queue limit must be positive");
        this.limit = limit;
    }
    void add(Write write, int capacity) {
        if (write.operationId == null || write.operationId.isEmpty() || write.bytes.length == 0
            || write.bytes.length > capacity) throw new IllegalArgumentException("Invalid write or capacity");
        if (pending.size() + (active == null ? 0 : 1) >= limit) throw new IllegalStateException("Write queue full");
        pending.addLast(write);
    }
    Write begin() {
        if (active == null) active = pending.pollFirst();
        return active;
    }
    Write active() { return active; }
    Write complete() { Write previous = active; active = null; return previous; }
    List<Write> cancel() {
        List<Write> cancelled = new ArrayList<>();
        if (active != null) cancelled.add(active);
        cancelled.addAll(pending);
        active = null;
        pending.clear();
        return cancelled;
    }
}
