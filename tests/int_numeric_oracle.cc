// GPL-2.0-or-later. Independent numeric CE interpreter oracle.
// Adapted from pinned Fallout 1 CE src/int/intrpret.cc operations on
// typed values and stacks; reads ONLY invented test bytecode, no game data.
#include <cstdint>
#include <cstring>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <limits>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

struct Value {
    bool floating = false;
    int32_t integer = 0;
    float fp = 0.0f;
};
static std::vector<uint8_t> bytes_from_hex(const std::string& hex) {
    if (hex.size() % 2) throw std::runtime_error("odd hex");
    std::vector<uint8_t> data;
    for (size_t p = 0; p < hex.size(); p += 2) {
        data.push_back(static_cast<uint8_t>(std::stoul(hex.substr(p, 2), nullptr, 16)));
    }
    return data;
}
static uint16_t read16(const std::vector<uint8_t>& b, size_t p) {
    if (p > b.size() || b.size() - p < 2) throw std::runtime_error("short opcode");
    return static_cast<uint16_t>((b[p] << 8) | b[p + 1]);
}
static uint32_t read32(const std::vector<uint8_t>& b, size_t p) {
    if (p > b.size() || b.size() - p < 4) throw std::runtime_error("short imm");
    return (uint32_t(b[p]) << 24) | (uint32_t(b[p + 1]) << 16) |
           (uint32_t(b[p + 2]) << 8) | uint32_t(b[p + 3]);
}
static Value integer(int32_t n) { return {false, n, 0.0f}; }
static Value floating(float n) { return {true, 0, n}; }
static float to_float(Value v) { return v.floating ? v.fp : static_cast<float>(v.integer); }
static bool truth(Value v) { return v.floating ? (std::signbit(v.fp) ? v.fp != 0.0f : v.fp != 0.0f) : v.integer != 0; }
static int32_t popint(std::vector<Value>& stack) {
    if (stack.empty()) throw std::runtime_error("underflow");
    Value v=stack.back(); stack.pop_back();
    if (v.floating) throw std::runtime_error("not int");
    return v.integer;
}
static Value pop(std::vector<Value>& stack) {
    if (stack.empty()) throw std::runtime_error("underflow");
    Value v=stack.back();stack.pop_back();return v;
}
static std::string execute(const std::vector<uint8_t>& b) {
    std::vector<Value> stack;
    size_t pc=0;
    bool stopped=false;
    for (int step=0; step<16 && !stopped; ++step) {
        uint16_t raw=read16(b,pc); pc+=2;
        if (!(raw&0x8000)) throw std::runtime_error("bad opcode");
        const auto opcode=raw&0x03FF;
        if (opcode==1) {
            uint32_t value=read32(b,pc);pc+=4;
            if(raw==0xC001)stack.push_back(integer(static_cast<int32_t>(value)));
            else if(raw==0xA001){float f;std::memcpy(&f,&value,4);stack.push_back(floating(f));}
            else throw std::runtime_error("bad push");
        } else if (opcode==0x11||opcode==0x10||opcode==0x0E) stopped=true;
        else if (opcode==0x1B){auto v=pop(stack);stack.push_back(v);stack.push_back(v);}
        else if (opcode==0x2F){
            auto cond=pop(stack);
            if (!truth(cond)) {
                int32_t dest=popint(stack);
                if (dest<0 || static_cast<size_t>(dest)>=b.size())throw std::runtime_error("bad jump");
                pc=static_cast<size_t>(dest);
            } else pop(stack);
        } else if (opcode==0x45){
            auto value=pop(stack);
            bool zero=value.floating ? (value.fp==0.0f && !std::signbit(value.fp)) : value.integer==0;
            stack.push_back(integer(zero?1:0));
        } else if(opcode==0x43)stack.push_back(integer(~popint(stack)));
        else if(opcode==0x39||opcode==0x3A||opcode==0x3B||opcode==0x3C||opcode==0x3D||opcode==0x37||opcode==0x3F||opcode==0x42) {
            auto rhs=pop(stack),lhs=pop(stack);
            if(opcode==0x39){
                if(!lhs.floating&&!rhs.floating){
                    int64_t sum=int64_t(lhs.integer)+rhs.integer;
                    if(sum>=std::numeric_limits<int32_t>::min()&&sum<=std::numeric_limits<int32_t>::max())
                        stack.push_back(integer(static_cast<int32_t>(sum)));
                    else stack.push_back(floating(to_float(lhs)+to_float(rhs)));
                } else stack.push_back(floating(to_float(lhs)+to_float(rhs)));
            } else if(opcode==0x3A) {
                if(!lhs.floating&&!rhs.floating)stack.push_back(integer(static_cast<int32_t>(uint32_t(lhs.integer)-uint32_t(rhs.integer))));
                else stack.push_back(floating(to_float(lhs)-to_float(rhs)));
            } else if(opcode==0x3B){
                if(!lhs.floating&&!rhs.floating)stack.push_back(integer(static_cast<int32_t>(uint32_t(lhs.integer)*uint32_t(rhs.integer))));
                else stack.push_back(floating(to_float(lhs)*to_float(rhs)));
            } else if(opcode==0x3C){
                if(to_float(rhs)==0.0f)throw std::runtime_error("zero");
                if(!lhs.floating&&!rhs.floating)stack.push_back(integer(lhs.integer/rhs.integer));
                else stack.push_back(floating(to_float(lhs)/to_float(rhs)));
            } else if(opcode==0x3D){
                if(lhs.floating||rhs.floating||rhs.integer==0)throw std::runtime_error("bad mod");
                stack.push_back(integer(lhs.integer%rhs.integer));
            } else if(opcode==0x37) stack.push_back(integer((to_float(lhs)<to_float(rhs))?1:0));
            else if(opcode==0x3F)stack.push_back(integer((truth(lhs)||truth(rhs))?1:0));
            else if(opcode==0x42){
                if(lhs.floating||rhs.floating)throw std::runtime_error("non int");
                stack.push_back(integer(lhs.integer^rhs.integer));
            }
        } else throw std::runtime_error("unimplemented");
    }
    if(!stopped)throw std::runtime_error("budget");
    std::ostringstream s;
    if(stack.empty())return "EMPTY";
    for(size_t i=0;i<stack.size();++i){
        if(i)s<<",";
        if(stack[i].floating)s<<"F:"<<std::fixed<<std::setprecision(6)<<stack[i].fp;
        else s<<"I:"<<stack[i].integer;
    }
    return s.str();
}
int main(int argc,char** argv) {
    if(argc!=2)return 2;
    std::ifstream cases(argv[1]);if(!cases)return 2;
    std::string name,hex;
    while(cases>>name>>hex) {
        try {std::cout<<name<<" "<<execute(bytes_from_hex(hex))<<"\n";}
        catch(const std::exception&) {std::cout<<name<<" ERR\n";}
    }
}