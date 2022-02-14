CKB-SDK provides the following operational interfaces:
> 1. Create CKB transactions to interact with Kabletop contracts.
> 2. Provide a build-in P2P module to enable easy implementation of a P2P client and server. (Albeit unstable at the moment and soon to be replaced with more stable P2P crates.)
> 3. Implement a simple wallet manager to manage plaintext keys, and will soon be upgraded to support generic key management tools like [WalletConnect](https://walletconnect.com/).

[kabletop-godot](https://github.com/ashuralyk/kabletop-godot) is written on `kabletop-ckb-sdk`, providing more productive interfaces to help developers write Kabletop games with ease using the Godot game engine.