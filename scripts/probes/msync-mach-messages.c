/* SPDX-License-Identifier: MIT
 * Bounded malformed-message probe for an owned disposable MSync prefix.
 * Build natively: clang -std=c11 -O2 -Wall -Wextra -Werror
 *                msync-mach-messages.c -o msync-mach-messages
 * Usage: probe --prefix PATH | --self-test-rights
 * Sends only invalid MSync packets to that prefix's exact bootstrap service;
 * no global lookup, signalling, process termination or valid object changes.
 * The harness must run the Windows sync probe afterward to prove survival.
 */
#include <mach/mach.h>
#include <servers/bootstrap.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

struct packet
{
    mach_msg_header_t header;
    mach_msg_body_t body;
    mach_msg_port_descriptor_t descriptor;
    unsigned int index;
};

static int self_test_rights(void)
{
    struct packet message = {0};
    struct { struct packet packet; mach_msg_max_trailer_t trailer; } received;
    semaphore_t sem = MACH_PORT_NULL;
    mach_port_t port = MACH_PORT_NULL;
    mach_port_urefs_t before = 0, after = 0;
    unsigned passed = 0;
    if (semaphore_create(mach_task_self(), &sem, SYNC_POLICY_FIFO, 0) != KERN_SUCCESS ||
        mach_port_allocate(mach_task_self(), MACH_PORT_RIGHT_RECEIVE, &port) != KERN_SUCCESS ||
        mach_port_insert_right(mach_task_self(), port, port, MACH_MSG_TYPE_MAKE_SEND) != KERN_SUCCESS)
        goto done;
    if (mach_port_get_refs(mach_task_self(), sem, MACH_PORT_RIGHT_SEND, &before) != KERN_SUCCESS)
        goto done;
    message.header.msgh_remote_port = port;
    message.header.msgh_bits = MACH_MSGH_BITS_REMOTE(MACH_MSG_TYPE_COPY_SEND) | MACH_MSGH_BITS_COMPLEX;
    message.header.msgh_size = offsetof(struct packet, index);
    message.body.msgh_descriptor_count = 1;
    message.descriptor.name = sem;
    message.descriptor.disposition = MACH_MSG_TYPE_COPY_SEND;
    message.descriptor.type = MACH_MSG_PORT_DESCRIPTOR;
    if (mach_msg(&message.header, MACH_SEND_MSG | MACH_SEND_TIMEOUT, message.header.msgh_size,
                 0, MACH_PORT_NULL, 1000, MACH_PORT_NULL) != MACH_MSG_SUCCESS) goto done;
    memset(&received, 0, sizeof(received));
    if (mach_msg(&received.packet.header, MACH_RCV_MSG | MACH_RCV_TIMEOUT, 0, sizeof(received),
                 port, 1000, MACH_PORT_NULL) != MACH_MSG_SUCCESS) goto done;
    passed += received.packet.descriptor.disposition == MACH_MSG_TYPE_PORT_SEND;
    passed += mach_port_get_refs(mach_task_self(), sem, MACH_PORT_RIGHT_SEND, &after) == KERN_SUCCESS
              && after == before + 1;
    passed += mach_port_mod_refs(mach_task_self(), sem, MACH_PORT_RIGHT_SEND, 2) == KERN_SUCCESS;
    passed += mach_port_get_refs(mach_task_self(), sem, MACH_PORT_RIGHT_SEND, &after) == KERN_SUCCESS
              && after == before + 3;
    mach_port_deallocate(mach_task_self(), sem);
    mach_port_deallocate(mach_task_self(), sem);
    mach_port_deallocate(mach_task_self(), sem);
    passed += mach_port_get_refs(mach_task_self(), sem, MACH_PORT_RIGHT_SEND, &after) == KERN_SUCCESS
              && after == before;
    passed += semaphore_signal(sem) == KERN_SUCCESS && semaphore_timedwait(sem,
              (mach_timespec_t){0, 1000000}) == KERN_SUCCESS;
done:
    if (MACH_PORT_VALID(sem)) semaphore_destroy(mach_task_self(), sem);
    if (MACH_PORT_VALID(port))
    {
        mach_port_deallocate(mach_task_self(), port);
        mach_port_mod_refs(mach_task_self(), port, MACH_PORT_RIGHT_RECEIVE, -1);
    }
    printf("{\"mach_send_right_contract_checks\":%u,\"expected\":6}\n", passed);
    return passed == 6 ? 0 : 1;
}

static int send_packet(struct packet *packet, unsigned count, unsigned index,
                       unsigned actual_indices, int descriptor, mach_port_t port,
                       semaphore_t semaphore)
{
    mach_msg_return_t result;
    memset(packet, 0, sizeof(*packet));
    packet->header.msgh_remote_port = port;
    packet->header.msgh_id = (0x1234 << 8) | count;
    packet->header.msgh_bits = MACH_MSGH_BITS_REMOTE(MACH_MSG_TYPE_COPY_SEND);
    if (descriptor)
    {
        packet->header.msgh_bits |= MACH_MSGH_BITS_COMPLEX;
        packet->body.msgh_descriptor_count = 1;
        packet->descriptor.name = semaphore;
        packet->descriptor.disposition = MACH_MSG_TYPE_COPY_SEND;
        packet->descriptor.type = MACH_MSG_PORT_DESCRIPTOR;
        packet->index = index;
        packet->header.msgh_size = (mach_msg_size_t)(offsetof(struct packet, index)
                                                   + actual_indices * sizeof(unsigned));
    }
    else
    {
        /* In a noncomplex unregister packet, indices immediately follow the
         * header, so the body's first word is the first invalid index. */
        packet->body.msgh_descriptor_count = index;
        packet->header.msgh_size = sizeof(mach_msg_header_t) + actual_indices * sizeof(unsigned);
    }
    result = mach_msg(&packet->header, MACH_SEND_MSG | MACH_SEND_TIMEOUT,
                      packet->header.msgh_size, 0, MACH_PORT_NULL, 1000, MACH_PORT_NULL);
    return result == MACH_MSG_SUCCESS;
}

int main(int argc, char **argv)
{
    struct stat st;
    uint64_t hash = 14695981039346656037ULL, values[3];
    unsigned value, byte, passed = 0;
    char name[29];
    struct packet packet;
    mach_port_t bootstrap = MACH_PORT_NULL, port = MACH_PORT_NULL;
    semaphore_t sem = MACH_PORT_NULL;
    if (argc == 2 && !strcmp(argv[1], "--self-test-rights")) return self_test_rights();
    if (argc != 3 || strcmp(argv[1], "--prefix") || stat(argv[2], &st)) return 2;
    values[0] = (uint64_t)st.st_dev;
    values[1] = (uint64_t)st.st_ino;
    values[2] = (uint64_t)getuid();
    for (value = 0; value < 3; ++value) for (byte = 0; byte < 8; ++byte)
    {
        hash ^= (values[value] >> (byte * 8)) & 0xff;
        hash *= 1099511628211ULL;
    }
    snprintf(name, sizeof(name), "wine-msync-%016llx", (unsigned long long)hash);
    if (task_get_special_port(mach_task_self(), TASK_BOOTSTRAP_PORT, &bootstrap) != KERN_SUCCESS)
        return 2;
    if (bootstrap_look_up(bootstrap, name, &port) != KERN_SUCCESS) return 2;
    if (semaphore_create(mach_task_self(), &sem, SYNC_POLICY_FIFO, 0) != KERN_SUCCESS) return 2;
    passed += send_packet(&packet, 66, 1, 1, 1, port, sem);
    passed += send_packet(&packet, 1, 1, 0, 1, port, sem);
    passed += send_packet(&packet, 1, 0x100000, 1, 1, port, sem);
    passed += send_packet(&packet, 1, 0, 1, 1, port, sem);
    passed += send_packet(&packet, 66, 1, 1, 0, port, sem);
    passed += send_packet(&packet, 1, 0x100000, 1, 0, port, sem);
    /* Invalid signal-only shared-memory index. */
    memset(&packet, 0, sizeof(packet));
    packet.header.msgh_remote_port = port;
    packet.header.msgh_bits = MACH_MSGH_BITS_REMOTE(MACH_MSG_TYPE_COPY_SEND);
    packet.header.msgh_id = 0x100000;
    packet.header.msgh_size = sizeof(mach_msg_header_t);
    passed += mach_msg(&packet.header, MACH_SEND_MSG | MACH_SEND_TIMEOUT,
                       packet.header.msgh_size, 0, MACH_PORT_NULL, 1000,
                       MACH_PORT_NULL) == MACH_MSG_SUCCESS;
    semaphore_destroy(mach_task_self(), sem);
    mach_port_deallocate(mach_task_self(), port);
    mach_port_deallocate(mach_task_self(), bootstrap);
    printf("{\"invalid_packets_sent\":%u,\"expected\":7,\"server_survival_requires_followup_probe\":true}\n",
           passed);
    return passed == 7 ? 0 : 1;
}
