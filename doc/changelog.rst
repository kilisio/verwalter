Verwalter Changes by Version
============================


.. _changelog-0.13.4:

Verwalter 0.13.4
----------------

* Feature: log of invoked actions added
  with logger ``verwalter::frontend::api::actions``


.. _changelog-0.13.3:

Verwalter 0.13.3
----------------

* bugfix: fix displaying actions on leader using default frontend


.. _changelog-0.13.2:

Verwalter 0.13.2
----------------

* bugfix: Fix link to alternate frontend in default frontend
* feature: add ``id`` field to graphql status
* bugfix: fix server list display in api frontend


.. _changelog-0.13.1:

Verwalter 0.13.1
----------------

* feature: add ``/v1/graphql`` endpoint with GraphQL API
* feature: add ``/v1/graphiql`` for poking with GraphQL API
* feature: default frontend now shows peers having errors
* feature: default frontend now shows full list of failing roles with the
  links to logs under the navigation bar


.. _changelog-0.13.0:

Verwalter 0.13.0
----------------

* breaking: all requests to ``/action`` and ``/wait_action`` now require
  ``Content-Type: application/json``
* feature: add support for ``query.wasm`` which might be used for
  overriding rendered roles and for custom queries
* feature: you can fetch current scheduler (and query) via API
  ``/v1/wasm/scheduler.wasm`` (only wasm scheduler though)


.. _changelog-0.12.1:

Verwalter 0.12.1
----------------

* Feature: add "node" variable to templates by default (in compatibility mode)


.. _changelog-0.12.0:

Verwalter 0.12.0
----------------

* We're preparing for list of roles and their variables be prepared
  by the wasm code in scheduler. This release only changes internals, preparing
  for that (we bump version to make a signal that things should be tested
  carefully).

.. _changelog-0.11.3:

Verwalter 0.11.3
----------------

* Feature: Added exerimental route ``/v1/leader-redirect-by-node-name/`` that
  returns redirect to a leader node
* Feature: Add a link to default frontend "common" frontend
* Bugfix: UI for 'Choice' variable type now works in api frontend


.. _changelog-0.11.2:

Verwalter 0.11.2
----------------

* bugfix: reset failures for the roles have been removed
* Feature: Add ``CleanFiles`` command


.. _changelog-0.11.1:

Verwalter 0.11.1
----------------

* feature: add ``Condition`` action which allows to execute an action if
  some files have been changed during executing other commands


.. _changelog-0.11.0:

Verwalter 0.11.0
----------------

* breaking: wasm scheduler requires returning object instead of tuple
* feature: new ``SplitText`` action, to deal with multiple generated
  files easily
* bugfix: wasm module will be reinitialized after panic
* bugfix: since verwalter 0.1.4 verwalter couldn't work as a single node
* breaking: serves ``/files/`` directory from static files


.. _changelog-0.10.4:

Verwalter 0.10.4
----------------

* feature: add an *experimental* ``--allow-minority-cluster`` option that
  allows verwalter to elect itself as a leader even if it sees less then
  N/2+1 nodes. I.e. in split-brain scenario two leaders might exist
  simultaneously which will then be merged. Note: this is a task of a
  specific scheduler to merge schedules appropriately.
* bugfix: additional css,js,fonts for alternative frontends were not
  served properly
* feature: allow to ``--default-frontend`` via CLI


.. _changelog-0.10.3:

Verwalter 0.10.3
----------------

* bugfix: timestamps in peer info now serialize as milliseconds since epoch
* wasm: add function to log panics
* wasm: add log/pow/exp functions needed for rust (actually llvm) build


.. _changelog-0.10.2:

Verwalter 0.10.2
----------------

* feature: upgrading trimmer to 0.3.6 allows to use escaping, dict and list
  literals in (.trm) templates
* Using ``wasmi`` instead of ``parity-wasm`` for interpreting wasm
* Initial routing for alternative frontends (``/~frontend-name/...`` urls)


.. _changelog-0.10.1:

Verwalter 0.10.1
----------------

* Timeout for incoming requests changed 10sec -> 2 min (mostly important to
  download larger logs)
* Template variables are passed to renderer using temporary file rather than
  command-line (working around limitations of sudo command line)



.. _changelog-0.10.0:

Verwalter 0.10.0
----------------

* Experimental webassembly scheduler support


.. _changelog-0.9.14:

Verwalter 0.9.14
----------------

* UI: fix chunk size in log tailer, mistakenly committed debugging version
* scheduler: if scheduler continue to fail for 5 min verwalter restarts on
  this node (this effectively elects a new leader)


.. _changelog-0.9.13:

Verwalter 0.9.13
----------------

* UI: add "Skip to End" button on log tail, skip by default on pressing "follow"


.. _changelog-0.9.12:

Verwalter 0.9.12
----------------

* Bugfix: fix crash on serving empty log
* Bugfix: JS error on the last step of api-frontend pipeline
* Log viewer leads to tail with correct offset


.. _changelog-0.9.11:

Verwalter 0.9.11
----------------

* Bugfix: Content-Range headers on logs were invalid
* Api-frontend: sorted server list
* Api-frontend: no "delete daemon" when update is active

.. _changelog-0.9.10:

Verwalter 0.9.10
----------------

* Add nicer log tailing UI and activate link in role log list
* Add some cantal metrics
* Bugfix: list of peers did not display correct timestamps

.. _changelog-0.9.9:

Verwalter 0.9.9
---------------

* Bugfix: external logs were not served properly
* Bugfix: when cantal fails for some time, verwalter could block


.. _changelog-0.9.8:

Verwalter 0.9.8
---------------

* Keeps few backups of old schedules
* Updates dependencies of frontend


.. _changelog-0.9.7:

Verwalter 0.9.7
---------------

* Bugfix: when request to cantal failed, verwalter would never reconnect


.. _changelog-0.9.6:

Verwalter 0.9.6
---------------

* Settings tweak: runtime load watchdog timeout is increased to 5 sec
* Bugfix: fix "rerender all roles" button (broken in 0.9.0)


.. _changelog-0.9.5:

Verwalter 0.9.5
---------------

* Bugfix: because we used unbuffered reading of runtime, it was too slow,
  effectively preventing scheduler to start on larger schedules
* Settings tweak: scheduler watchdog timeout is increased to 5 sec


.. _changelog-0.9.4:

Verwalter 0.9.4
---------------

* Bugfix: follower was unable to render templates (only leader)


.. _changelog-0.9.3:

Verwalter 0.9.3
---------------

* Peer info (known since, last ping) is now visible again (broken in 0.9.0)


.. _changelog-0.9.2:

Verwalter 0.9.2
---------------

* Fix bug in showing old schedule at ``/api/v1/schedule`` api
* Logs now served by newer library, so bigger subset of requests supported
  (last modified, no range, ...)

.. _changelog-0.9.1:

Verwalter 0.9.1
---------------

* Release packaging fixes and few dependencies upgraded


.. _changelog-0.9.0:

Verwalter 0.9.0
---------------

The mayor change in this version of scheduler that we migrated from rotor
network stack to tokio network stack. This is technically changes nothing
from user point of view. But we also decided to drop/fix rarely used functions
to make release more quick:

1. Dropped ``/api/v1/scheduler`` API, most useful info is now in
   ``/api/v1/status`` API
2. Some keys in status are changed
3. No metrics support any more, we'll reveal them in subsequent releases
   (we need more performant API in cantal for that)

Yes, we still use ``/v1`` and don't guarantee backwards compatibility
between 0.x releases. That would be a major pain.
