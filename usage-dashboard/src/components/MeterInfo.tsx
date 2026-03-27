'use client';

import { MeterData } from '@/types';
import { Battery, Zap, Clock, Wallet } from 'lucide-react';

interface MeterInfoProps {
  meterData: MeterData;
}

export default function MeterInfo({ meterData }: MeterInfoProps) {
  const formatAddress = (address: string) => {
    return address.length > 20 ? `${address.substring(0, 10)}...${address.substring(address.length - 6)}` : address;
  };

  const formatBalance = (balance: number) => {
    return balance.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 6 });
  };

  const lastUpdateTime = new Date(meterData.lastUpdate).toLocaleString();

  return (
    <div className="chart-container">
      <h2 className="text-xl font-bold text-gray-900 mb-6">Meter Information</h2>
      
      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        {/* Meter Details */}
        <div className="space-y-4">
          <div className="flex items-center space-x-3">
            <Zap className="w-5 h-5 text-primary-600" />
            <div>
              <p className="text-sm text-gray-600">Meter ID</p>
              <p className="font-medium text-gray-900">{meterData.id}</p>
            </div>
          </div>
          
          <div className="flex items-center space-x-3">
            <Wallet className="w-5 h-5 text-primary-600" />
            <div>
              <p className="text-sm text-gray-600">User Wallet</p>
              <p className="font-mono text-sm text-gray-900">{formatAddress(meterData.user)}</p>
            </div>
          </div>
          
          <div className="flex items-center space-x-3">
            <Battery className="w-5 h-5 text-primary-600" />
            <div>
              <p className="text-sm text-gray-600">Provider</p>
              <p className="font-mono text-sm text-gray-900">{formatAddress(meterData.provider)}</p>
            </div>
          </div>
          
          <div className="flex items-center space-x-3">
            <Clock className="w-5 h-5 text-primary-600" />
            <div>
              <p className="text-sm text-gray-600">Last Update</p>
              <p className="font-medium text-gray-900">{lastUpdateTime}</p>
            </div>
          </div>
        </div>
        
        {/* Rate and Balance Info */}
        <div className="space-y-4">
          <div className="bg-gray-50 rounded-lg p-4">
            <h3 className="text-sm font-medium text-gray-700 mb-3">Rate Information</h3>
            <div className="space-y-2">
              <div className="flex justify-between">
                <span className="text-sm text-gray-600">Off-Peak Rate:</span>
                <span className="font-medium text-green-600">{meterData.offPeakRate} XLM/kWh</span>
              </div>
              <div className="flex justify-between">
                <span className="text-sm text-gray-600">Peak Rate:</span>
                <span className="font-medium text-red-600">{meterData.peakRate} XLM/kWh</span>
              </div>
              <div className="flex justify-between">
                <span className="text-sm text-gray-600">Peak Multiplier:</span>
                <span className="font-medium text-gray-900">{(meterData.peakRate / meterData.offPeakRate).toFixed(1)}x</span>
              </div>
            </div>
          </div>
          
          <div className="bg-primary-50 rounded-lg p-4">
            <h3 className="text-sm font-medium text-primary-700 mb-3">Account Status</h3>
            <div className="space-y-2">
              <div className="flex justify-between">
                <span className="text-sm text-primary-600">Current Balance:</span>
                <span className="font-medium text-primary-900">{formatBalance(meterData.balance)} XLM</span>
              </div>
              <div className="flex justify-between">
                <span className="text-sm text-primary-600">Total Usage:</span>
                <span className="font-medium text-primary-900">{meterData.totalUsage.toFixed(3)} kWh</span>
              </div>
              <div className="flex justify-between">
                <span className="text-sm text-primary-600">Total Spend:</span>
                <span className="font-medium text-primary-900">{formatBalance(meterData.totalSpend)} XLM</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
