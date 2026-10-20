import express from "express";

// Import before the app. Express 5 keeps no mount path on a router's layer, so the inventory
// (routes.test.ts) records it here: every router.use call (app.use goes through one), with its path.
export const Mounts = new Map<unknown, string[]>();

function Record(Original: (...Args: any[]) => any){
    return function(this: unknown, ...Args: any[]){
        const Path = typeof Args[0] === "string" ? Args[0] : "/";

        for(const Fn of Args.flat()){
            if(typeof Fn === "function"){
                Mounts.set(Fn, [...(Mounts.get(Fn) ?? []), Path]);
            }
        }

        return Original.apply(this, Args);
    };
}

// A router is a function whose prototype is a Router instance; the methods are one level further up.
const RouterProto = Object.getPrototypeOf(Object.getPrototypeOf(express.Router()));
RouterProto.use = Record(RouterProto.use);
