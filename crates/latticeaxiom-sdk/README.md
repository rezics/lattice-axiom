# latticeaxiom-sdk

`latticeaxiom-sdk` is the code-bound registration foundation frozen by ADR
0023. Attribute macros describe components and row-kernel systems, while an
explicit `registration_ir!` invocation lists every generated row. There is no
linker inventory, constructor registration, runtime global registry, native
loader, or project-owned ECS facade.

The generated artifacts are engine-independent data. A later host adapter may
turn `StaticAdapterPlan` into a normal typed Bevy system and must compare
Bevy's actual access with the canonical `SystemSignature`. A portable native
bridge may consume `DynamicBatchShimPlan`; this crate neither loads nor calls a
native library.
