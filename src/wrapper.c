#include <stdlib.h>
#include <stdio.h>
#include <evpaxos.h>
#include <signal.h>

static void handle_sigint(int sig, short ev, void* arg);

void start_learner(const char* config, deliver_function f, void* arg) {
	struct event* sig;
	struct evlearner* lea;
	struct event_base* base;

	base = event_base_new();
	lea = evlearner_init(config, f, arg, base);
	if (lea == NULL) {
		printf("Could not start the learner!\n");
		exit(1);
	}

	sig = evsignal_new(base, SIGINT, handle_sigint, base);
	evsignal_add(sig, NULL);

	signal(SIGPIPE, SIG_IGN);
	event_base_dispatch(base);

	event_free(sig);
	evlearner_free(lea);
	event_base_free(base);
}

static void
handle_sigint(int sig, short ev, void* arg)
{
    (void) ev; // ignore ev
	struct event_base* base = arg;
	printf("Caught signal %d\n", sig);
	event_base_loopexit(base, NULL);
}
