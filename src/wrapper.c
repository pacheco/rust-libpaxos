#include <stdlib.h>
#include <stdio.h>
#include <evpaxos.h>
#include <signal.h>
#include <msgpack.h>

/* callback for the msgpack packer to write data */
typedef int (*writefn)(void* arg, const char* buf, size_t len);

static void msgpack_pack_string(msgpack_packer* p, const char* buffer, int len);

void start_learner(const char* config, deliver_function f, void* arg, unsigned starting_iid)
{
	struct evlearner* lea;
	struct event_base* base;

	base = event_base_new();
	lea = evlearner_init(config, f, arg, base);
	if (lea == NULL) {
		printf("Could not start the learner!\n");
		exit(1);
	}

    evlearner_set_instance_id(lea, starting_iid);

	signal(SIGPIPE, SIG_IGN);
	event_base_dispatch(base);

	evlearner_free(lea);
	event_base_free(base);
}

void serialize_submit(const char* value, size_t len, writefn write, void *write_arg)
{
    msgpack_packer* p;
	p = msgpack_packer_new(write_arg, write);
	msgpack_pack_array(p, 2);
	msgpack_pack_int32(p, 8); // FIXME: libpaxos does not expose the message constants
	msgpack_pack_string(p, value, len);
	msgpack_packer_free(p);
}

static void msgpack_pack_string(msgpack_packer* p, const char* buffer, int len)
{
	#if MSGPACK_VERSION_MAJOR > 0
	msgpack_pack_bin(p, len);
	msgpack_pack_bin_body(p, buffer, len);
	#else
	msgpack_pack_raw(p, len);
	msgpack_pack_raw_body(p, buffer, len);
	#endif
}
