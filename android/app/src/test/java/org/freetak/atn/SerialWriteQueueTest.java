package org.freetak.atn;

import org.junit.Test;
import static org.junit.Assert.*;

public final class SerialWriteQueueTest {
    @Test public void holdsSingleActiveAndPreservesOrder() {
        SerialWriteQueue queue = new SerialWriteQueue(2);
        queue.add(new SerialWriteQueue.Write("one", new byte[]{1}), 20);
        queue.add(new SerialWriteQueue.Write("two", new byte[]{2}), 20);
        assertEquals("one", queue.begin().operationId);
        assertEquals("one", queue.begin().operationId);
        queue.complete();
        assertEquals("two", queue.begin().operationId);
    }
    @Test public void cancellationRemovesActiveAndPendingWithoutReplay() {
        SerialWriteQueue queue = new SerialWriteQueue(2);
        queue.add(new SerialWriteQueue.Write("one", new byte[]{1}), 20);
        queue.add(new SerialWriteQueue.Write("two", new byte[]{2}), 20);
        queue.begin();
        assertEquals(2, queue.cancel().size());
        assertNull(queue.begin());
    }
    @Test public void rejectsOverflowAndCapacityBeforeMutation() {
        SerialWriteQueue queue = new SerialWriteQueue(1);
        try { queue.add(new SerialWriteQueue.Write("oversize", new byte[21]), 20); fail(); }
        catch (IllegalArgumentException expected) { assertNull(queue.begin()); }
        queue.add(new SerialWriteQueue.Write("one", new byte[20]), 20);
        queue.begin();
        try { queue.add(new SerialWriteQueue.Write("overflow", new byte[]{2}), 20); fail(); }
        catch (IllegalStateException expected) { assertEquals("one", queue.active().operationId); }
    }
    @Test public void ownsCopyOfCallerBytes() {
        byte[] input = {1};
        SerialWriteQueue.Write write = new SerialWriteQueue.Write("one", input);
        input[0] = 2;
        assertEquals(1, write.bytes[0]);
    }
}
