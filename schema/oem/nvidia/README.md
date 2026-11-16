<!--
SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
SPDX-License-Identifier: Apache-2.0

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
-->

# NVIDIA OEM schemas

The `*_v1.xml` files are copied verbatim from
[`NVIDIA/bmcweb`](https://github.com/NVIDIA/bmcweb/tree/145ee5400553630637f01734915abde5915751a7/redfish-core/schema/oem/nvidia/csdl)
at commit `145ee5400553630637f01734915abde5915751a7`.
Keep these files unchanged. Local descriptions of firmware fields and actions
missing from the published schemas belong in adjacent `ExtNvidia*.xml` files,
such as `ExtNvidiaComputerSystem.xml` and `ExtNvidiaManager.xml`.

## Local extensions

Local complex types use ordinary CSDL inheritance in a separate namespace:

```xml
<Schema xmlns="http://docs.oasis-open.org/odata/ns/edm"
        Namespace="NvidiaNetworkAdapter.NvRedfish">
  <ComplexType Name="NvidiaNetworkAdapter"
               BaseType="NvidiaNetworkAdapter.v1_3_0.NvidiaNetworkAdapter">
    <Property Name="BaseMAC" Type="Edm.String">
      <Annotation Term="OData.Permissions" EnumMember="OData.Permission/Read"/>
    </Property>
  </ComplexType>
</Schema>
```

The existing compiler follows inheritance and flattens the generated models.
Keep the upstream schema family as the namespace prefix so namespace pruning
preserves the Rust module names. The Manager extension is nested below
`NvidiaManager.v1_9_0` to preserve the generated model's existing versioned path.
`BmcRShim` belongs to that extended Manager; the upstream base Manager and its
separate FabricManager branch do not gain BlueField properties.

New types and actions also live in separate namespaces. `ClearNVRAM` uses
`NvidiaUpdateService.NvRedfish`; the compiler already uses the root namespace
for action names on the wire, preserving `#NvidiaUpdateService.ClearNVRAM`.
The BlueField actions retain their advertised `Mode`, `HostRshim` and `SOC`
namespaces.

`ExtNvidiaChassis.xml` defines the complete reset enum, including the
BlueField-4 values, and a `Reset` action using it. CSDL enums do not support
inheritance. The manifest selects this local action and explicitly lists the
other upstream chassis actions, excluding the original `Reset`. When updating
upstream, reconcile the enum and check for newly added chassis actions.

List each extension file beside its upstream file in `redfish/features.toml`.
The build also offers unselected upstream and extension files for resolution;
this keeps dependencies available when only one service feature is enabled.

## Updating from upstream

1. Replace the top-level `*_v1.xml` files with the upstream CSDL files, leaving
   the local `ExtNvidia*.xml` files intact.
2. Update the pinned commit here and in `redfish/features.toml`. Adjust feature
   roots if upstream adds or removes files.
3. Point each extension's `BaseType` at the appropriate new upstream type.
   If upstream now defines a local field, type or action, remove or adapt the
   redundant local declaration.
4. Run the existing NVIDIA integration tests and check the generated API for
   changes to names, fields and inheritance branches.
